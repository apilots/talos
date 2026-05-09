# Runtime Env And First-Run Bootstrap Design

Status: active
Owner: agent
Created: 2026-05-08
Updated: 2026-05-08

## 背景

当前 env/bootstrap 逻辑分散在 Rust、Desktop main 和配置写入路径中：Rust 入口只 `dotenv()` 当前目录，Desktop 单独解析 `~/.talos/.env` 并执行 login shell，`Config::load()` 又从进程环境解析 `${VAR}`。结果是 Desktop、CLI、TUI、server 的启动语义不一致，并且新环境可能因为缺 `talos.yaml` 或 API key 起不来。

目标是把启动能力和模型可用性拆开：Talos 必须能启动、注册、登录和打开 Settings；只有发起模型调用时才要求 active provider 已配置好 API key。

## 目标模型

环境来源只保留两类：

- 系统进程环境变量。
- `${TALOS_HOME:-~/.talos}/.env`。

统一加载点在 `talos_config`，所有入口复用同一套 loader。Desktop 不再解析 `.env`，不再执行 login shell，不再根据 `talos.yaml` placeholder 动态 export 环境变量。

优先级固定：

```text
system TALOS_* env > ~/.talos/.env
talos.yaml owns non-secret config, env only resolves ${TALOS_*} placeholders
```

命名约束：

- `TALOS_...`：runtime secret 和普通运行时变量，例如 `TALOS_GATEWAY_JWT_SECRET`、`TALOS_LLM_ANTHROPIC_API_KEY`。
- 新模板不再使用无前缀 secret，例如 `ANTHROPIC_API_KEY`。
- 不做旧 env 名称兼容，也不保留结构化 env 覆盖。已有本地 `.env` 需要用户或迁移脚本直接改成新的 `TALOS_` key。

## First-Run Flow

```text
Desktop / CLI / server
        |
        v
talos_config bootstrap
        |
        +-- ensure ~/.talos/{conf,logs,data}
        +-- copy embedded talos.yaml.template if missing
        +-- create/update ~/.talos/.env with TALOS_GATEWAY_JWT_SECRET if missing
        |
        v
load talos.yaml + filtered env snapshot
        |
        +-- startup validation: auth/storage/runtime shape
        +-- model readiness: provider/model/api key status
        |
        v
runtime ready, model may be setup_required
        |
        v
register/login -> Settings -> save provider key -> reload readiness
```

默认 `talos.yaml.template`：

- `gateway.auth: true`
- `gateway.allow_register: true`
- `gateway.jwt_secret: ${TALOS_GATEWAY_JWT_SECRET}`
- active provider 默认 `anthropic`
- API key 使用 `${TALOS_LLM_ANTHROPIC_API_KEY}`

## Secret Write Flow

用户在 Settings 填写 API key 时，不把 key 写入 `talos.yaml`。写入规则：

```text
UI settings save
        |
        v
ConfigService update LLM config
        |
        +-- literal api_key -> write ~/.talos/.env provider variable
        +-- YAML llm section keeps ${TALOS_LLM_*_API_KEY}
        +-- clear api_key -> remove provider variable from ~/.talos/.env
        |
        v
reload env snapshot + model readiness
```

`config.read` 不应向 renderer 返回明文 secret。Settings 只需要知道 key 是否已配置，可通过 readiness/status 字段表达。

## 清理旧 Env 实现

本重构是替换式改造，不保留过渡加载路径。需要删除已有的多套 env 逻辑，避免新旧路径同时存在导致优先级不可解释。

必须删除：

- Rust 入口中的当前目录 `dotenv()` 语义，不再从进程工作目录读取 `.env`。
- Desktop main 的 `~/.talos/.env` TS parser。
- Desktop packaged macOS login shell env loading。
- Desktop 根据 `talos.yaml` placeholder 动态 export 环境变量再执行 `env` 的逻辑。
- `readConfigPlaceholderNames` 这类为了 shell env 合并存在的 helper。
- `TALOS__...` 结构化配置覆盖以及对应的 JSON path override/parser。
- 无前缀 provider key 的模板引用，例如 `${ANTHROPIC_API_KEY}`、`${OPENAI_API_KEY}`、`${BAILIAN_API_KEY}`。
- 启动期把 provider API key unresolved placeholder 当作 fatal config error 的校验。

保留并收敛：

- `TALOS_HOME` 仍用于定位 runtime home。
- `TALOS_LOCAL_LAUNCH_TOKEN` 仍只用于本地 app-server 握手。
- `~/.talos/.env` 由 Rust config 层统一读写，只接收 `TALOS_` 前缀 key。

## Validation Split

启动校验只检查服务可启动的前置条件：

- YAML 结构和 required keys。
- auth 相关配置可用于启动。
- storage/log/runtime path 合法。
- agent/workspace 基础配置合法。

模型 readiness 检查：

- active provider enabled。
- active model 存在于 provider model catalog。
- required base URL 存在。
- required API key 已解析且非空。

缺 API key 的结果是 `setup_required`，不是进程退出。

## 接口清单

| 模块 | 接口 | 职责 |
| --- | --- | --- |
| `talos_config` | `RuntimeEnvLoader` | 读取并合并 process env 与 `~/.talos/.env`，输出过滤后的 env snapshot。 |
| `talos_config` | `FirstRunBootstrap` | 创建基础目录、复制模板、生成本地 secret。 |
| `talos_config` | `EnvSecretStore` | 读写 `~/.talos/.env`，用于 API key 和本地 secret。 |
| `talos_config` | `Config::load_bootstrapped()` | 首启 bootstrap + config load + startup validation。 |
| `talos_config` | `ModelReadiness` | 返回 active provider 的可用性和缺失项。 |
| `application-service` | `ConfigService` | 保存 provider 配置时把 secret 写入 `.env`，YAML 只保存 placeholder。 |
| Desktop main | `resolveAppServerEnvironment` | 收缩为只传必要 process env，不再解析 shell 或 `.env`。 |

## 核心 API 伪代码

```rust
struct RuntimeEnv {
    values: HashMap<String, String>,
}

impl RuntimeEnv {
    fn load(paths: &TalosPaths, process: &ProcessEnv) -> TalosResult<Self> {
        let dotenv = EnvSecretStore::new(paths.env_file()).read_filtered("TALOS_")?;
        Ok(Self::merge(dotenv, process.filtered("TALOS_")))
    }
}

impl FirstRunBootstrap {
    fn ensure(paths: &TalosPaths) -> TalosResult<()> {
        paths.ensure_base_dirs()?;
        copy_template_if_missing(paths.config_file)?;
        EnvSecretStore::new(paths.env_file())
            .ensure_random("TALOS_GATEWAY_JWT_SECRET")?;
        Ok(())
    }
}

impl Config {
    fn load_bootstrapped() -> TalosResult<(Config, ModelReadiness)> {
        let paths = TalosPaths::resolve();
        FirstRunBootstrap::ensure(&paths)?;
        let env = RuntimeEnv::load(&paths, &ProcessEnv::current())?;
        let config = ConfigLoader::load_with_env(paths.config_file, env)?;
        validate_startup(&config)?;
        let readiness = ModelReadiness::from_config(&config);
        Ok((config, readiness))
    }
}
```

## 边界

- 不做 OS installer。首次初始化发生在应用启动时。
- 不在 YAML 写入 secret 明文。
- 不再依赖用户 shell 配置来发现 key；用户可通过系统环境变量或 Settings 写入 `.env`。
- 不兼容旧无前缀变量；已有 `.env` 直接迁移成新 `TALOS_` key。

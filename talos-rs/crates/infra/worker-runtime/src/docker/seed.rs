use bollard::{
    exec::StartExecResults,
    models::{ContainerCreateBody, ExecConfig, HostConfig},
    query_parameters::{CreateContainerOptions, RemoveContainerOptions},
};
use futures_util::TryStreamExt;
use tokio::io::AsyncWriteExt;

use super::{engine_error, DockerEngine, Names, HOME_PATH, NO_NEW_PRIVILEGES, WORKER_USER};
use crate::{RuntimeError, SeedFile, SeedMode, WorkerSpec};

// The helper runs without a network and uses the same unprivileged UID as the Worker.
// Secret bytes travel over exec stdin. Positional arguments contain only file metadata.
const WRITE_FILE: &str = r#"
set -eu
cd "$1"
target=$2
while [ "${target#*/}" != "$target" ]; do
    part=${target%%/*}
    test ! -L "$part"
    if [ ! -d "$part" ]; then
        mkdir -m 700 -- "$part"
        sync .
    fi
    cd -- "$part"
    target=${target#*/}
done
test ! -L "$target"
if [ -e "$target" ]; then
    test -f "$target"
fi
temp=$(mktemp -- .talos-seed.XXXXXXXX)
trap 'rm -f -- "$temp"' EXIT
cat > "$temp"
test "$(wc -c < "$temp")" -eq "$4"
sync "$temp"
if [ "$3" = replace ]; then
    mv -T -- "$temp" "$target"
elif [ ! -e "$target" ]; then
    ln -T -- "$temp" "$target"
fi
sync .
"#;

pub(super) fn validate_paths(files: &[SeedFile]) -> Result<Vec<&str>, RuntimeError> {
    files
        .iter()
        .map(|file| {
            file.path
                .to_str()
                .filter(|path| {
                    path.split('/').all(|part| {
                        !part.is_empty()
                            && part != "."
                            && part != ".."
                            && part
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                    })
                })
                .ok_or(RuntimeError::InvalidSpec(
                    "Home file paths must contain only normal relative components",
                ))
        })
        .collect()
}

impl DockerEngine {
    pub(super) async fn seed_home(
        &self,
        spec: &WorkerSpec,
        names: &Names,
        files: &[SeedFile],
        paths: &[&str],
        mode: SeedMode,
    ) -> Result<(), RuntimeError> {
        self.ensure_image(&spec.image).await?;
        self.remove_seeder(names).await?;
        let helper = self
            .client
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(names.seeder()),
                    ..Default::default()
                }),
                ContainerCreateBody {
                    image: Some(spec.image.clone()),
                    user: Some(WORKER_USER.into()),
                    entrypoint: Some(vec!["/bin/sleep".into()]),
                    cmd: Some(vec!["120".into()]),
                    labels: Some(names.labels("seeder")),
                    host_config: Some(HostConfig {
                        mounts: Some(vec![names.home_mount()]),
                        network_mode: Some("none".into()),
                        readonly_rootfs: Some(true),
                        security_opt: Some(vec![NO_NEW_PRIVILEGES.into()]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .map_err(engine_error("create Worker Home seeder"))?;
        let seeded = async {
            self.client
                .start_container(&helper.id, None)
                .await
                .map_err(engine_error("start Worker Home seeder"))?;
            for (file, path) in files.iter().zip(paths) {
                self.write_seed(&helper.id, path, &file.contents, mode)
                    .await?;
            }
            Ok(())
        }
        .await;
        let cleanup = self.remove_seed_container(&helper.id).await;
        seeded.and(cleanup)
    }

    async fn write_seed(
        &self,
        helper_id: &str,
        path: &str,
        contents: &[u8],
        mode: SeedMode,
    ) -> Result<(), RuntimeError> {
        let mode = match mode {
            SeedMode::CreateMissing => "create",
            SeedMode::Replace => "replace",
        };
        let length = contents.len().to_string();
        let command = [
            "/bin/bash",
            "-c",
            WRITE_FILE,
            "talos-seed",
            HOME_PATH,
            path,
            mode,
            &length,
        ];
        let exec = self
            .client
            .create_exec(
                helper_id,
                ExecConfig {
                    attach_stdin: Some(true),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    cmd: Some(command.into_iter().map(str::to_owned).collect()),
                    ..Default::default()
                },
            )
            .await
            .map_err(engine_error("create Worker Home file operation"))?;
        let StartExecResults::Attached { output, mut input } = self
            .client
            .start_exec(&exec.id, None)
            .await
            .map_err(engine_error("attach Worker Home file operation"))?
        else {
            return Err(RuntimeError::UnexpectedState {
                resource: "Home file operation",
            });
        };
        input
            .write_all(contents)
            .await
            .map_err(RuntimeError::SeedIo)?;
        input.shutdown().await.map_err(RuntimeError::SeedIo)?;
        output
            .try_for_each(|_| async { Ok(()) })
            .await
            .map_err(engine_error("complete Worker Home file operation"))?;
        let result = self
            .client
            .inspect_exec(&exec.id)
            .await
            .map_err(engine_error("inspect Worker Home file operation"))?;
        if result.exit_code != Some(0) {
            return Err(RuntimeError::UnexpectedState {
                resource: "Home file write",
            });
        }
        Ok(())
    }

    pub(super) async fn remove_seeder(&self, names: &Names) -> Result<(), RuntimeError> {
        if let Some(container) = self.container(&names.seeder()).await? {
            let labels = container
                .config
                .as_ref()
                .and_then(|config| config.labels.as_ref());
            names.verify_labels(labels, "seeder")?;
            names.verify_mount(&container)?;
            let id = container.id.ok_or(RuntimeError::UnexpectedState {
                resource: "seeder ID",
            })?;
            self.remove_seed_container(&id).await?;
        }
        Ok(())
    }

    async fn remove_seed_container(&self, id: &str) -> Result<(), RuntimeError> {
        self.remove_container(
            id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await
    }
}

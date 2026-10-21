export const errorResources = {
  en: {
    browserCapability: { nativeHost: "This Computer does not share its native desktop.", notBundled: "The required browser runtime is not bundled on this Computer.", hostPolicy: "The host policy does not allow this browser operation.", unsupportedVersion: "This browser runtime version does not support this operation.", clientMismatch: "Open the browser from its own Desktop host." },
    detail: {
      computerDeletionActive: "Finish or stop this Computer's active tasks before removing its registration.",
      computerRegistrationRemoved: "This Computer registration was removed. Use Connect a computer to enroll again.",
      computerDeletionUnverified: "Unable to verify this Computer's tasks. Reconnect it or wait for its access lease to expire, then retry.",
      modelNotConfigured: 'Model "{{model}}" is not configured for provider "{{provider}}".',
      attachmentCountExceeded: "This request contains {{actual}} attachments; the limit is {{limit}}.",
    },
  },
  zh: {
    browserCapability: { nativeHost: "这台 Computer 不共享本机桌面。", notBundled: "这台 Computer 未内置所需的浏览器运行环境。", hostPolicy: "宿主策略不允许这项浏览器操作。", unsupportedVersion: "当前浏览器运行环境版本不支持这项操作。", clientMismatch: "请在浏览器所属的 Desktop 客户端打开。" },
    detail: {
      computerDeletionActive: "此 Computer 还有未结束的任务，请先完成或停止任务，再移除注册。",
      computerRegistrationRemoved: "此 Computer 的注册已移除，请通过「连接 Computer」主动重新注册。",
      computerDeletionUnverified: "暂时无法确认此 Computer 的任务状态，请恢复连接或等待访问租约到期后重试。",
      modelNotConfigured: '提供方“{{provider}}”未配置模型“{{model}}”。',
      attachmentCountExceeded: "本次请求包含 {{actual}} 个附件，上限为 {{limit}} 个。",
    },
  },
} as const;

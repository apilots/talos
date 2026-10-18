export const errorResources = {
  en: {
    detail: {
      computerDeletionActive: "Finish or stop this Computer's active tasks before removing its registration.",
      computerRegistrationRemoved: "This Computer registration was removed. Use Connect a computer to enroll again.",
      computerDeletionUnverified: "Unable to verify this Computer's tasks. Reconnect it or wait for its access lease to expire, then retry.",
      modelNotConfigured: 'Model "{{model}}" is not configured for provider "{{provider}}".',
      attachmentCountExceeded: "This request contains {{actual}} attachments; the limit is {{limit}}.",
    },
  },
  zh: {
    detail: {
      computerDeletionActive: "此 Computer 还有未结束的任务，请先完成或停止任务，再移除注册。",
      computerRegistrationRemoved: "此 Computer 的注册已移除，请通过「连接 Computer」主动重新注册。",
      computerDeletionUnverified: "暂时无法确认此 Computer 的任务状态，请恢复连接或等待访问租约到期后重试。",
      modelNotConfigured: '提供方“{{provider}}”未配置模型“{{model}}”。',
      attachmentCountExceeded: "本次请求包含 {{actual}} 个附件，上限为 {{limit}} 个。",
    },
  },
} as const;

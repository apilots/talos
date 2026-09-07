export const errorResources = {
  en: {
    detail: {
      modelNotConfigured: 'Model "{{model}}" is not configured for provider "{{provider}}".',
      attachmentCountExceeded: "This request contains {{actual}} attachments; the limit is {{limit}}.",
    },
  },
  zh: {
    detail: {
      modelNotConfigured: '提供方“{{provider}}”未配置模型“{{model}}”。',
      attachmentCountExceeded: "本次请求包含 {{actual}} 个附件，上限为 {{limit}} 个。",
    },
  },
} as const;

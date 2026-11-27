/** Resolve aliases and relative imports before applying the existing layer policy. */
module.exports = {
  forbidden: [
    {
      name: "shared-does-not-import-features",
      severity: "error",
      from: { path: "^web/src/shared/", pathNot: "^web/src/shared/(store|test)/" },
      to: { path: "^web/src/features/" },
    },
    {
      name: "widgets-receive-feature-data",
      severity: "error",
      from: { path: "^web/src/widgets/" },
      to: { path: "^web/src/features/" },
    },
    {
      name: "composition-uses-feature-barrels",
      severity: "error",
      from: { path: "^web/src/(app|pages)/" },
      to: { path: "^web/src/features/[^/]+/", pathNot: "^web/src/features/[^/]+/index\\.ts$" },
    },
  ],
  options: {
    tsConfig: { fileName: "web/tsconfig.json" },
    tsPreCompilationDeps: true,
    doNotFollow: { path: "node_modules" },
    exclude: { path: "\\.test\\.|/__tests__/|\\.test-support\\.|\\.test-utils\\." },
    enhancedResolveOptions: { exportsFields: ["exports"], conditionNames: ["import", "types", "default"] },
  },
};

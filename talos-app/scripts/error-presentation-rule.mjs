/** Prevent presentation from bypassing the shared error/localization boundary. */
export const errorPresentationRule = {
  meta: { type: "problem", schema: [], messages: {
    presenter: "Render errors through apiErrorMessage/userVisibleErrorMessage; keep stored API errors structured.",
  } },
  create(context) {
    const services = context.sourceCode.parserServices;
    const checker = services.program.getTypeChecker();
    const isError = (type) => {
      if (type.isUnion()) return type.types.some(isError);
      return type.aliasSymbol?.name === "ApiError" ||
        Boolean(type.getProperty("message") && type.getProperty("name") && type.getProperty("stack"));
    };
    return {
      CallExpression(node) {
        if (!['apiErrorMessage', 'userVisibleErrorMessage'].includes(node.callee.name)) return;
        const parent = node.parent;
        if (parent.type !== "CallExpression") return;
        const setter = parent.callee.name ?? parent.callee.property?.name;
        if (/^set.*(?:Error|Failure)$/.test(setter ?? "")) {
          context.report({ node, messageId: "presenter" });
        }
      },
      ObjectPattern(node) {
        const original = services.esTreeNodeToTSNodeMap.get(node);
        if (!isError(checker.getTypeAtLocation(original))) return;
        for (const property of node.properties) {
          if (property.type !== "Property") continue;
          const key = property.computed ? property.key.value : property.key.name ?? property.key.value;
          if (key === "message") context.report({ node: property, messageId: "presenter" });
        }
      },
      MemberExpression(node) {
        const property = node.computed ? node.property.value : node.property.name;
        if (property !== "message") return;
        const original = services.esTreeNodeToTSNodeMap.get(node.object);
        if (isError(checker.getTypeAtLocation(original))) {
          context.report({ node, messageId: "presenter" });
        }
      },
    };
  },
};

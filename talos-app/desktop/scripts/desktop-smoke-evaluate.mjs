/** Evaluate smoke actions through CDP without losing structured application codes. */
export async function evaluate(cdp, expression) {
  const result = await cdp.call("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result.exceptionDetails) {
    const remote = result.exceptionDetails.exception;
    const detail = remote?.objectId
      ? await cdp.call("Runtime.callFunctionOn", {
          objectId: remote.objectId,
          functionDeclaration: "function () { return { code: this.code, message: this.message }; }",
          returnByValue: true,
        })
      : null;
    const error = detail?.result.value;
    throw Object.assign(
      new Error(error?.message ?? result.exceptionDetails.text ?? "Runtime.evaluate failed."),
      { code: error?.code },
    );
  }
  return result.result.value;
}

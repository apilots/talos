import type {
  DynamicToolArguments,
  JsonContentValue,
  ToolArguments,
  ToolExecutionResult,
} from "./generated/app-protocol";

export function jsonContentValue(value: JsonContentValue): unknown {
  return value;
}

export function toolArgumentsValue(value: ToolArguments): unknown {
  return value;
}

export function dynamicToolArgumentsValue(value: DynamicToolArguments): unknown {
  return value;
}

export function toolExecutionResultValue(value: ToolExecutionResult): unknown {
  return value;
}

import { ComputerDesktopButton } from "./ComputerDesktop";
import { useComputer } from "./hooks";

export function SessionDesktopShortcut({ nodeId }: { nodeId: string }) {
  const computer = useComputer(nodeId);
  if (!computer.isSuccess || !computer.data.available) return null;

  return <ComputerDesktopButton nodeId={nodeId} online shortcut />;
}

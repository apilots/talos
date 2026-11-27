import { ComputerDesktopButton } from "./ComputerDesktop";
import { useComputer } from "./hooks";

export function SessionDesktopShortcut({ nodeId }: { nodeId: string }) {
  const computer = useComputer(nodeId);
  if (!computer.data?.available || computer.data.managedWorkerPhase != null) return null;

  return <ComputerDesktopButton nodeId={nodeId} online shortcut />;
}

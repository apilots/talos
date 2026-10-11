import { useNavigate } from "react-router-dom";
import { logoutUser } from "../api";
import { useAppStore } from "@/shared/store/app-store";

export function useLogout() {
  const navigate = useNavigate();
  return async () => {
    try { await logoutUser(); } finally {
      useAppStore.getState().setConnection({ authToken: "", authStatus: "logged_out", authRole: "user", accountType: "Free", authUsername: "Anonymous", userId: "anonymous" });
      navigate("/auth/logout");
    }
  };
}

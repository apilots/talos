export type AccountType = "Free" | "Plus" | "Pro" | "Max";

export function accountTypeFromWire(value: Lowercase<AccountType>): AccountType {
  switch (value) {
    case "free":
      return "Free";
    case "plus":
      return "Plus";
    case "pro":
      return "Pro";
    case "max":
      return "Max";
  }
}
/** Authentication lifecycle shared by connection state and presentation props. */
export type AuthStatus =
  | "unknown"
  | "loading"
  | "error"
  | "authenticated"
  | "unauthenticated"
  | "logged_out"
  | "open";

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

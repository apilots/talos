import { UserCircle } from "lucide-react";

interface ProfileIdentityProps {
  avatarLabel: string;
  accountTypeLabel: string;
  displayName: string;
  openMode: boolean;
  username: string;
}

export function ProfileIdentity({
  avatarLabel,
  accountTypeLabel,
  displayName,
  openMode,
  username,
}: ProfileIdentityProps) {
  return (
    <section className="text-center">
      <div className="mx-auto flex size-24 items-center justify-center overflow-hidden rounded-full border border-border/80 bg-card text-3xl font-semibold text-ink">
        {openMode ? avatarLabel : <UserCircle aria-hidden="true" size={48} />}
      </div>
      <h2 className="mt-6 text-[2rem] font-semibold leading-tight tracking-normal text-ink text-balance">
        {displayName}
      </h2>
      <div className="mt-2 flex items-center justify-center gap-2 text-sm text-muted">
        <span>{username}</span>
        <span aria-hidden="true">·</span>
        <span className="rounded-full border border-border/80 px-2 py-0.5 text-xs leading-5">
          {accountTypeLabel}
        </span>
      </div>
    </section>
  );
}

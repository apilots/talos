import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { AdminCreateUserRequest, AdminPatchUserRequest, UserDto, UserRole } from "@talos/sdk";
import { useTranslation } from "react-i18next";
import { PencilLine, Trash2, X } from "lucide-react";

import { createUser, deleteUser, listUsers, patchUser } from "@/features/auth";
import { useConnectionSettings } from "@/features/connection";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

interface UserFormState {
  username: string;
  email: string;
  password: string;
  role: UserRole;
  disabled: boolean;
}

type UserModalMode = "create" | "edit";

const emptyCreateForm: UserFormState = {
  username: "",
  email: "",
  password: "",
  role: "user",
  disabled: false,
};

function buildEditForm(user: UserDto): UserFormState {
  return {
    username: user.username,
    email: user.email ?? "",
    password: "",
    role: user.role,
    disabled: user.disabled,
  };
}

function parseErrorMessage(error: unknown): string {
  if (error instanceof Error && "body" in error) {
    const body = String((error as { body?: string }).body ?? "");
    if (body) {
      try {
        const parsed = JSON.parse(body) as { message?: string };
        if (parsed.message) {
          return parsed.message;
        }
      } catch {
        return body;
      }
    }
  }
  if (error instanceof Error && error.message) {
    return error.message;
  }
  return "Request failed.";
}

function normalizeForm(form: UserFormState) {
  return {
    username: form.username.trim(),
    email: form.email.trim(),
    password: form.password.trim(),
    role: form.role,
    disabled: form.disabled,
  };
}

export function AdminUsersSection() {
  const { t } = useTranslation(["admin", "auth", "common"]);
  const { connection } = useConnectionSettings();
  const queryClient = useQueryClient();
  const [modalMode, setModalMode] = useState<UserModalMode | null>(null);
  const [selectedUser, setSelectedUser] = useState<UserDto | null>(null);
  const [form, setForm] = useState<UserFormState>(emptyCreateForm);
  const [formError, setFormError] = useState("");
  const [actionError, setActionError] = useState("");

  const usersQueryKey = useMemo(
    () => ["admin-users", connection.serverUrl, connection.authToken, connection.userId] as const,
    [connection.authToken, connection.serverUrl, connection.userId],
  );

  const usersQuery = useQuery({
    queryKey: usersQueryKey,
    queryFn: listUsers,
  });

  const createMutation = useMutation({
    mutationFn: (request: AdminCreateUserRequest) => createUser(request),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: usersQueryKey });
      closeModal();
    },
    onError: (error) => {
      setFormError(parseErrorMessage(error));
    },
  });

  const editMutation = useMutation({
    mutationFn: ({ userId, request }: { userId: string; request: AdminPatchUserRequest }) =>
      patchUser(userId, request),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: usersQueryKey });
      closeModal();
    },
    onError: (error) => {
      setFormError(parseErrorMessage(error));
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (userId: string) => deleteUser(userId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: usersQueryKey });
      setActionError("");
    },
    onError: (error) => {
      setActionError(parseErrorMessage(error));
    },
  });

  function openCreateModal() {
    setModalMode("create");
    setSelectedUser(null);
    setForm(emptyCreateForm);
    setFormError("");
    setActionError("");
  }

  function openEditModal(user: UserDto) {
    setModalMode("edit");
    setSelectedUser(user);
    setForm(buildEditForm(user));
    setFormError("");
    setActionError("");
  }

  function closeModal() {
    setModalMode(null);
    setSelectedUser(null);
    setForm(emptyCreateForm);
    setFormError("");
  }

  function updateForm<K extends keyof UserFormState>(key: K, value: UserFormState[K]) {
    setForm((current) => ({ ...current, [key]: value }));
    setFormError("");
  }

  async function submitModal() {
    const normalized = normalizeForm(form);
    if (modalMode === "create") {
      if (!normalized.username || !normalized.email || !normalized.password) {
        setFormError(t("auth:requiredFields"));
        return;
      }
      await createMutation.mutateAsync({
        username: normalized.username,
        email: normalized.email,
        password: normalized.password,
        role: normalized.role,
        disabled: normalized.disabled,
      });
      return;
    }

    if (!selectedUser) {
      return;
    }
    if (!normalized.username) {
      setFormError(t("auth:requiredFields"));
      return;
    }

    const request: AdminPatchUserRequest = {
      username: normalized.username,
      role: normalized.role,
      disabled: normalized.disabled,
    };
    if (normalized.password) {
      request.password = normalized.password;
    }

    await editMutation.mutateAsync({
      userId: selectedUser.user_id,
      request,
    });
  }

  async function handleDelete(user: UserDto) {
    setActionError("");
    if (!window.confirm(t("usersPanel.deleteConfirm", { username: user.username }))) {
      return;
    }
    await deleteMutation.mutateAsync(user.user_id);
  }

  const isModalOpen = modalMode !== null;
  const isEditing = modalMode === "edit";
  const isSubmitting = createMutation.isPending || editMutation.isPending;

  return (
    <>
      <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
        <div className="mb-4 flex items-center justify-between gap-3">
          <div>
            <div className="text-sm font-medium text-ink">{t("usersPanel.title")}</div>
            <div className="mt-1 text-sm text-muted">{t("usersPanel.detail")}</div>
          </div>
          <Button onClick={openCreateModal} type="button" variant="secondary">
            {t("usersPanel.create")}
          </Button>
        </div>

        {actionError ? (
          <div className="mb-4 rounded-[1rem] border border-danger/20 bg-danger/8 px-4 py-3 text-sm text-danger">
            {t("usersPanel.actionErrorPrefix", { message: actionError })}
          </div>
        ) : null}

        {usersQuery.isLoading ? (
          <div className="py-8 text-sm text-muted">{t("common:loading")}</div>
        ) : usersQuery.isError ? (
          <div className="py-8 text-sm text-danger">{parseErrorMessage(usersQuery.error)}</div>
        ) : (
          <div className="space-y-3">
            <div className="grid grid-cols-[minmax(0,1.1fr)_minmax(0,1.35fr)_120px_120px_160px] gap-4 px-4 text-xs uppercase tracking-[0.16em] text-muted">
              <div>{t("usersPanel.username")}</div>
              <div>{t("usersPanel.email")}</div>
              <div>{t("usersPanel.role")}</div>
              <div>{t("usersPanel.status")}</div>
              <div className="text-right">{t("usersPanel.actions")}</div>
            </div>
            {usersQuery.data && usersQuery.data.length > 0 ? (
              usersQuery.data.map((user) => {
                const deletingThisUser = deleteMutation.isPending && deleteMutation.variables === user.user_id;
                return (
                  <div
                    className="grid grid-cols-[minmax(0,1.1fr)_minmax(0,1.35fr)_120px_120px_160px] items-center gap-4 rounded-[1.2rem] border border-border/80 bg-background/45 px-4 py-4"
                    key={user.user_id}
                  >
                    <div className="min-w-0">
                      <div className="truncate font-medium text-ink">{user.username}</div>
                    </div>
                    <div className="min-w-0 text-sm text-muted">{user.email ?? "—"}</div>
                    <div className="text-sm text-muted">
                      {user.role === "admin" ? t("usersPanel.roles.admin") : t("usersPanel.roles.member")}
                    </div>
                    <div className="text-sm text-muted">
                      {user.disabled ? t("usersPanel.disabled") : t("usersPanel.active")}
                    </div>
                    <div className="flex justify-end">
                      <div className="inline-flex items-center gap-1 rounded-[999px] border border-border/75 bg-surface/70 p-1">
                        <button
                          className="inline-flex h-9 items-center gap-1.5 rounded-[999px] px-3 text-sm text-muted transition hover:bg-background hover:text-ink"
                          onClick={() => openEditModal(user)}
                          type="button"
                        >
                          <PencilLine size={15} />
                          {t("usersPanel.edit")}
                        </button>
                        <button
                          className="inline-flex h-9 items-center gap-1.5 rounded-[999px] px-3 text-sm text-danger transition hover:bg-danger/10"
                          disabled={deletingThisUser}
                          onClick={() => void handleDelete(user)}
                          type="button"
                        >
                          <Trash2 size={15} />
                          {deletingThisUser ? t("usersPanel.deleting") : t("usersPanel.delete")}
                        </button>
                      </div>
                    </div>
                  </div>
                );
              })
            ) : (
              <div className="rounded-[1.2rem] border border-dashed border-border/80 px-4 py-8 text-sm text-muted">
                {t("usersPanel.empty")}
              </div>
            )}
          </div>
        )}
      </Panel>

      {isModalOpen ? (
        <div className="fixed inset-0 z-40 flex items-center justify-center bg-[rgba(28,22,16,0.46)] px-6 py-10">
          <div className="absolute inset-0" onClick={closeModal} />
          <Panel className="relative z-10 w-full max-w-[620px] rounded-[1.5rem] border-border/75 bg-card p-6 shadow-[0_24px_80px_rgba(69,55,34,0.18)]">
            <div className="flex items-start justify-between gap-6">
              <div>
                <div className="text-lg font-semibold text-ink">
                  {isEditing ? t("usersPanel.editTitle") : t("usersPanel.createTitle")}
                </div>
                <div className="mt-1 text-sm text-muted">
                  {isEditing ? t("usersPanel.modalDetailEdit") : t("usersPanel.modalDetailCreate")}
                </div>
              </div>
              <button
                className="inline-flex h-10 w-10 items-center justify-center rounded-full text-muted transition hover:bg-surface hover:text-ink"
                onClick={closeModal}
                type="button"
              >
                <X size={18} />
              </button>
            </div>

            <div className="mt-6 grid gap-4 md:grid-cols-2">
              <div>
                <label className="mb-2 block text-xs uppercase tracking-[0.18em] text-muted">
                  {t("usersPanel.username")}
                </label>
                <Input
                  onChange={(event) => updateForm("username", event.target.value)}
                  value={form.username}
                />
              </div>

              <div>
                <label className="mb-2 block text-xs uppercase tracking-[0.18em] text-muted">
                  {t("usersPanel.email")}
                </label>
                <Input
                  disabled={isEditing}
                  onChange={(event) => updateForm("email", event.target.value)}
                  type="email"
                  value={form.email}
                />
                {isEditing ? (
                  <div className="mt-2 text-xs text-muted">{t("usersPanel.emailLockedHint")}</div>
                ) : null}
              </div>

              <div>
                <label className="mb-2 block text-xs uppercase tracking-[0.18em] text-muted">
                  {isEditing ? t("usersPanel.passwordOptional") : t("usersPanel.password")}
                </label>
                <Input
                  onChange={(event) => updateForm("password", event.target.value)}
                  type="password"
                  value={form.password}
                />
              </div>

              <div>
                <label className="mb-2 block text-xs uppercase tracking-[0.18em] text-muted">
                  {t("usersPanel.role")}
                </label>
                <select
                  className="w-full rounded-[var(--radius-control)] border border-border/85 bg-surface/88 px-4 py-3 text-sm text-ink outline-none transition focus:border-brand/60 focus:bg-background/92"
                  onChange={(event) => updateForm("role", event.target.value as UserRole)}
                  value={form.role}
                >
                  <option value="user">{t("usersPanel.roles.member")}</option>
                  <option value="admin">{t("usersPanel.roles.admin")}</option>
                </select>
              </div>
            </div>

            <label className="mt-5 inline-flex items-center gap-2 text-sm text-muted">
              <input
                checked={form.disabled}
                className="h-4 w-4 accent-[var(--color-brand)]"
                onChange={(event) => updateForm("disabled", event.target.checked)}
                type="checkbox"
              />
              {t("usersPanel.disabled")}
            </label>

            {formError ? <div className="mt-4 text-sm text-danger">{formError}</div> : null}

            <div className="mt-6 flex items-center justify-end gap-2">
              <Button onClick={closeModal} type="button" variant="ghost">
                {t("common:cancel")}
              </Button>
              <Button disabled={isSubmitting} onClick={() => void submitModal()} type="button">
                {isSubmitting
                  ? t("usersPanel.saving")
                  : isEditing
                    ? t("usersPanel.editSubmit")
                    : t("usersPanel.createSubmit")}
              </Button>
            </div>
          </Panel>
        </div>
      ) : null}
    </>
  );
}

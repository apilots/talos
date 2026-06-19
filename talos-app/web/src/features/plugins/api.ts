import { getTalosClient } from "@/shared/api/client";
import type { ProtocolSkillSummary } from "@talos/sdk-browser";

export async function listSkills(includeDisabled = true) {
  return getTalosClient().skills.list({ include_disabled: includeDisabled });
}

export async function readSkillDocument(skill: ProtocolSkillSummary) {
  return getTalosClient().skills.read({
    selector: {
      name: skill.name,
      path: skill.path,
    },
  });
}

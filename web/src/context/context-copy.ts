import type { MessageKey } from "../i18n/translate";
import type {
  SkillSkipReason,
  SkillTier,
} from "../protocol/generated/protocol";

/** Each skip reason in plain words; a reason added to the protocol fails the typecheck until it has copy. */
export const REASONS: Readonly<Record<SkillSkipReason, MessageKey>> = {
  missing_manifest: "context.reasonMissingManifest",
  bad_front_matter: "context.reasonBadFrontMatter",
  name_mismatch: "context.reasonNameMismatch",
  too_large: "context.reasonTooLarge",
  symlink: "context.reasonSymlink",
  not_utf8: "context.reasonNotUtf8",
  shadowed: "context.reasonShadowed",
  over_limit: "context.reasonOverLimit",
  unreadable: "context.reasonUnreadable",
};

/** A reason from an older or newer runtime still reads as plain words (UI-GUIDELINES §14). */
export const reasonKey = (reason: string): MessageKey =>
  Object.hasOwn(REASONS, reason)
    ? REASONS[reason as SkillSkipReason]
    : "context.reasonUnknown";

/** Whose a skill is, in the reader's words. */
export const TIERS: Readonly<Record<SkillTier, MessageKey>> = {
  install: "context.tierInstall",
  user: "context.tierUser",
};

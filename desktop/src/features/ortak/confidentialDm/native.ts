import { invoke } from "@tauri-apps/api/core";
import type { NativeDm } from "./types";

/** Purpose-specific commands only; no provider, signer or generic decrypt input. */
export const nativeDm: NativeDm = (command, args) => {
  if (
    ![
      "encrypted_dm_begin",
      "encrypted_dm_close",
      "encrypted_dm_authority",
      "encrypted_dm_open",
      "encrypted_dm_save_draft",
      "encrypted_dm_prepare",
      "encrypted_dm_publish",
      "encrypted_dm_retire",
    ].includes(command)
  )
    return Promise.reject(new Error("Unsupported encrypted DM operation"));
  return invoke(command, args);
};

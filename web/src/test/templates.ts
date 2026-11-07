/** Synthetic catalogue shared by environment UI tests. */
export const templates = {
  items: [
    {
      name: "workstation",
      display_name: "Workstation",
      capabilities: ["shell", "python", "node", "build", "browser"],
      is_default: true,
      description: null,
      cpus: 2,
      memory_mib: 4096,
    },
    {
      name: "minimal",
      display_name: "Minimal",
      capabilities: ["shell", "python"],
      is_default: false,
      description: null,
      cpus: 1,
      memory_mib: 512,
    },
  ],
} as const;

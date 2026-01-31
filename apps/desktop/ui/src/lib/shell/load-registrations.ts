// Pulls in every `lib/views/**/register.ts` so a module registers its screens (see screens.ts) without
// anyone editing a shared file. Imported once, by main.ts, before the first render.
const modules = import.meta.glob('../views/**/register.ts', { eager: true });

export const registrationCount = Object.keys(modules).length;

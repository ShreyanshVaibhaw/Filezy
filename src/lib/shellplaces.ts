export const DEVICES = "::Devices";
export const SHELL_PREFIX = "::Shell:";
export const SHELL_PC = "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}";
export const shellLocation = (parsing: string) => SHELL_PREFIX + encodeURIComponent(parsing);
export const isShellLocation = (path: string) => path === DEVICES || path.startsWith(SHELL_PREFIX);
export function shellParsing(path: string): string {
  return path === DEVICES ? SHELL_PC : decodeURIComponent(path.slice(SHELL_PREFIX.length));
}
export interface ShellEntry { name: string; path: string; folder: boolean; copy: boolean }
export interface ShellFolder { name: string; parent: string | null; items: ShellEntry[]; capped: boolean }

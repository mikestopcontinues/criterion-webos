import { constants } from "node:fs";
import { lstat, open, realpath } from "node:fs/promises";
import { resolve } from "node:path";

/** Read one fixed build input without following links or allocating beyond its admitted size. */
export async function readInput(path: string, maxBytes: number, minBytes: 0 | 1 = 1): Promise<Buffer> {
  const absolute = resolve(path);
  const before = await lstat(absolute);
  if (!before.isFile() || before.isSymbolicLink() || before.size < minBytes || before.size > maxBytes || await realpath(absolute) !== absolute) throw new Error("invalidInput");
  const file = await open(absolute, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const stat = await file.stat();
    if (!stat.isFile() || stat.size !== before.size || stat.size > maxBytes) throw new Error("invalidInput");
    const bytes = Buffer.alloc(stat.size + 1);
    let offset = 0;
    while (offset < bytes.length) {
      const result = await file.read(bytes, offset, bytes.length - offset, offset);
      if (result.bytesRead === 0) break;
      offset += result.bytesRead;
    }
    if (offset !== stat.size || (await file.stat()).size !== stat.size) throw new Error("invalidInput");
    return bytes.subarray(0, offset);
  } finally { await file.close(); }
}

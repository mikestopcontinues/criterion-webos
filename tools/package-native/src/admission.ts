export const APP_ID = "com.mikestopcontinues.criterion.unofficial";
export const VERSION = "0.1.0";
export const MAX_EXECUTABLE_BYTES = 32 * 1024 * 1024;
export const MAX_IPK_BYTES = MAX_EXECUTABLE_BYTES + 2 * 1024 * 1024;
export function admitExecutable(bytes: Buffer): void {
  const invalid = () => { throw new Error("invalidExecutable"); };
  if (bytes.length < 52 || bytes.length > MAX_EXECUTABLE_BYTES || !bytes.subarray(0, 7).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]))
    || bytes[7] !== 0 || ![2, 3].includes(bytes.readUInt16LE(16)) || bytes.readUInt16LE(18) !== 40 || bytes.readUInt32LE(20) !== 1
    || bytes.readUInt16LE(40) !== 52 || (bytes.readUInt32LE(36) & 0xff000000) !== 0x05000000
    || (bytes.readUInt32LE(36) & 0x600) !== 0x200) invalid();
  const offset = bytes.readUInt32LE(28);
  const count = bytes.readUInt16LE(44);
  if (bytes.readUInt16LE(42) !== 32 || count < 1 || count > 64 || offset < 52 || offset + count * 32 > bytes.length) invalid();
  let interpreter = false;
  let stack = false;
  let code = false;
  for (let index = 0; index < count; index += 1) {
    const header = offset + index * 32;
    const type = bytes.readUInt32LE(header);
    const start = bytes.readUInt32LE(header + 4);
    const size = bytes.readUInt32LE(header + 16);
    const flags = bytes.readUInt32LE(header + 24);
    if (start + size > bytes.length) invalid();
    if (type === 1 && size > 0 && (flags & 1) !== 0) code = true;
    if (type === 3) {
      if (interpreter || !bytes.subarray(start, start + size).equals(Buffer.from("/lib/ld-linux.so.3\0", "ascii"))) invalid();
      interpreter = true;
    }
    if (type === 0x6474e551) {
      if (stack || flags !== 6) invalid();
      stack = true;
    }
  }
  if (!interpreter || !stack || !code) invalid();
}

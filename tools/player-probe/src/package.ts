/** Build-time admission only. No package or service request can choose a binary. */
export function admitBroker(bytes: Buffer): void {
  if (bytes.length < 52 || bytes.length > 2 * 1024 * 1024 || !bytes.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))
    || bytes[4] !== 1 || bytes[5] !== 1 || bytes[6] !== 1 || ![2, 3].includes(bytes.readUInt16LE(16))
    || bytes.readUInt16LE(18) !== 40 || bytes.readUInt32LE(20) !== 1 || bytes.readUInt16LE(40) !== 52
    || (bytes.readUInt32LE(36) & 0xff000000) !== 0x05000000 || (bytes.readUInt32LE(36) & 0x400) !== 0) {
    throw new Error("invalidBroker");
  }
}

/** Only the three fixed ar members emitted by the pinned packaging CLI are admitted. */
export function ipkMembers(bytes: Buffer, maxBytes = 2 * 1024 * 1024): ReadonlyMap<string, Buffer> {
  if (!Number.isSafeInteger(maxBytes) || maxBytes < 8 || maxBytes > 64 * 1024 * 1024
    || bytes.length > maxBytes || bytes.subarray(0, 8).toString() !== "!<arch>\n") throw new Error("invalidIpk");
  let offset = 8;
  const members = new Map<string, Buffer>();
  while (offset < bytes.length) {
    if (offset + 60 > bytes.length) throw new Error("invalidIpk");
    const headerBytes = bytes.subarray(offset, offset + 60);
    if (headerBytes.some((byte) => byte > 0x7f)) throw new Error("invalidIpk");
    const header = headerBytes.toString("ascii");
    const name = header.slice(0, 16).trim().replace(/\/$/, "");
    const length = header.slice(48, 58).trim();
    if (header.slice(58) !== "`\n" || !/^[0-9]{1,10}$/.test(length) || name !== ["debian-binary", "control.tar.gz", "data.tar.gz"][members.size]) throw new Error("invalidIpk");
    const size = Number(length);
    offset += 60;
    if (offset + size > bytes.length) throw new Error("invalidIpk");
    members.set(name, bytes.subarray(offset, offset + size));
    if (size % 2 && bytes[offset + size] !== 10) throw new Error("invalidIpk");
    offset += size + size % 2;
  }
  const data = members.get("data.tar.gz");
  if (offset !== bytes.length || members.size !== 3 || members.get("debian-binary")?.toString() !== "2.0\n" || !data) throw new Error("invalidIpk");
  return members;
}

export function ipkData(bytes: Buffer, maxBytes = 2 * 1024 * 1024): Buffer {
  const data = ipkMembers(bytes, maxBytes).get("data.tar.gz");
  if (!data) throw new Error("invalidIpk");
  return data;
}

import { gzipSync } from "node:zlib";

export type TarEntry = { name: string; bytes: Buffer; mode: number; type?: string };
/** Literal USTAR test frames; production uses GNU tar and never this serializer. */
export function tarFixture(entries: TarEntry[]): Buffer {
  const chunks: Buffer[] = [];
  for (const entry of entries) {
    const h = Buffer.alloc(512);
    const slash = entry.name.lastIndexOf("/"); const long = entry.name.length > 100;
    h.write(long ? entry.name.slice(slash + 1) : entry.name, 0, "ascii");
    if (long) h.write(entry.name.slice(0, slash), 345, "ascii");
    h.write(`${entry.mode.toString(8).padStart(7, "0")}\0`, 100, "ascii");
    h.write("0000000\0", 108, "ascii"); h.write("0000000\0", 116, "ascii");
    h.write(`${entry.bytes.length.toString(8).padStart(11, "0")}\0`, 124, "ascii");
    h.write("00000000000\0", 136, "ascii"); h.fill(32, 148, 156);
    h.write(entry.type ?? "0", 156, "ascii"); h.write("ustar\0", 257, "ascii"); h.write("00", 263, "ascii");
    h.write(`${[...h].reduce((sum, byte) => sum + byte, 0).toString(8).padStart(6, "0")}\0 `, 148, "ascii");
    chunks.push(h, entry.bytes, Buffer.alloc((512 - entry.bytes.length % 512) % 512));
  }
  chunks.push(Buffer.alloc(1024)); return Buffer.concat(chunks);
}
export function ipkFixture(data: Buffer, control: Buffer): Buffer {
  const chunks: Buffer[] = [Buffer.from("!<arch>\n")];
  for (const [name, body] of [["debian-binary", Buffer.from("2.0\n")], ["control.tar.gz", gzipSync(control)], ["data.tar.gz", gzipSync(data)]] as const) {
    chunks.push(Buffer.from(`${name.padEnd(16)}${"0".padEnd(12)}${"0".padEnd(6)}${"0".padEnd(6)}${"100644".padEnd(8)}${String(body.length).padEnd(10)}\x60\n`), body);
    if (body.length % 2) chunks.push(Buffer.from("\n"));
  }
  return Buffer.concat(chunks);
}

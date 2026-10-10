import { dirname, resolve } from "node:path";
import { packageNativeMain } from "./build.js";
import { MAX_EXECUTABLE_BYTES } from "./src/admission.js";
import type { NativeMainExport, PackagingExecution } from "./src/contract.js";
import { readInput } from "./src/input.js";

export type PackagingFiles = Readonly<{
  sourceRoot: string; outputDirectory: string; executablePath: string; receiptPath: string;
}>;

/** Inert packaging-image entry; the canonical producer invokes it explicitly. */
export async function packageMainFromFiles(input: PackagingFiles, execution: PackagingExecution): Promise<NativeMainExport> {
  const { sourceRoot, outputDirectory, executablePath, receiptPath } = input;
  if (resolve(sourceRoot) !== sourceRoot || resolve(outputDirectory) !== outputDirectory
    || !/^\/workspace\/\.local\/native-package\/(?:input|[a-z]{1,64})\/criterion-unofficial$/.test(executablePath)
    || receiptPath !== dirname(executablePath) + "/build-receipt.json") throw new Error("invalidMainPackagingFiles");
  let previous = -Infinity;
  const guard = () => {
    const current = execution.now();
    if (!Number.isSafeInteger(current) || current < 0 || current < previous || !Number.isSafeInteger(execution.deadlineMs)
      || current >= execution.deadlineMs) throw new Error("packagingDeadline");
    previous = current;
  };
  guard(); const executable = await readInput(executablePath, MAX_EXECUTABLE_BYTES); guard();
  const buildReceipt = await readInput(receiptPath, 256 * 1024); guard();
  return packageNativeMain({ sourceRoot, outputDirectory, executable, buildReceipt }, execution);
}

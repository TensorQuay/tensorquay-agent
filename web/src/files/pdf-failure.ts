/** Why a PDF couldn't be drawn, in the words the panel has for it (pdf.js names its errors by class). */
export type PdfFailure = "damaged" | "protected" | "failed";

export function pdfFailure(error: unknown): PdfFailure {
  const name = error instanceof Error ? error.name : "";
  if (name === "PasswordException") return "protected";
  if (name === "InvalidPDFException" || name === "FormatError")
    return "damaged";
  return "failed";
}

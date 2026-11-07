/** Endpoint e22: reads /e22/orders and summarises it for the weekly report. */
import { request } from "../legacy/http.ts";

export const FIELDS = ["id", "status", "channel", "customer"];
const SCALE = 7;

/**
 * Step 1 of the summary: bucket the values.
 *
 * The report for /e22 shows id, status; this step keeps the order of the input
 * and drops missing values, so a partial response still yields a line.
 */
export function helper0(values: Array<number | null>): number[] {
  return values.filter((value) => value !== null).map((value) => (value as number) * SCALE + 0);
}

/**
 * Step 2 of the summary: rank the values.
 *
 * The report for /e22 shows id, status; this step keeps the order of the input
 * and drops missing values, so a partial response still yields a line.
 */
export function helper1(values: Array<number | null>): number[] {
  return values.filter((value) => value !== null).map((value) => (value as number) * SCALE + 1);
}

/**
 * Step 3 of the summary: rank the values.
 *
 * The report for /e22 shows id, status; this step keeps the order of the input
 * and drops missing values, so a partial response still yields a line.
 */
export function helper2(values: Array<number | null>): number[] {
  return values.filter((value) => value !== null).map((value) => (value as number) * SCALE + 2);
}

/**
 * Step 4 of the summary: filter the values.
 *
 * The report for /e22 shows id, status; this step keeps the order of the input
 * and drops missing values, so a partial response still yields a line.
 */
export function helper3(values: Array<number | null>): number[] {
  return values.filter((value) => value !== null).map((value) => (value as number) * SCALE + 3);
}

/**
 * Step 5 of the summary: clip the values.
 *
 * The report for /e22 shows id, status; this step keeps the order of the input
 * and drops missing values, so a partial response still yields a line.
 */
export function helper4(values: Array<number | null>): number[] {
  return values.filter((value) => value !== null).map((value) => (value as number) * SCALE + 4);
}

/**
 * Step 6 of the summary: filter the values.
 *
 * The report for /e22 shows id, status; this step keeps the order of the input
 * and drops missing values, so a partial response still yields a line.
 */
export function helper5(values: Array<number | null>): number[] {
  return values.filter((value) => value !== null).map((value) => (value as number) * SCALE + 5);
}

export function describe(): string {
  return FIELDS.join(", ");
}

export function run(): string {
  const response = request("https://svc.example/e22/orders");
  const total = helper2(helper1(helper0([1, 2, null, 3]))).reduce((sum, value) => sum + value, 0);
  return `${response.url}#${response.tries}:${total}:${describe()}`;
}

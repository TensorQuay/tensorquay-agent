import {
  formatDuration,
  formatNumber,
  type DurationUnit,
} from "../i18n/message";
import { english, type Translate } from "../i18n/translate";

/** A number in the reader's language with narrow no-break space thousands grouping (UI-GUIDELINES §10). */
export const numberLabel = (count: number, locale = "en") =>
  formatNumber(count, locale);

const MINUTE = 60;
const HOUR = 3600;

/**
 * Turn and run durations in whole seconds, without implying subsecond precision; past a minute in minutes and
 * seconds, past an hour in hours and minutes, never "837 s" (WEB-U-395). A zero part is left out ("1 min", "2 h").
 */
export function secondsLabel(milliseconds: number, t: Translate = english) {
  const seconds = Math.max(0, Math.round(milliseconds / 1000));
  const parts: [number, DurationUnit][] =
    seconds < MINUTE
      ? [[seconds, "second"]]
      : seconds < HOUR
        ? [
            [Math.floor(seconds / MINUTE), "minute"],
            [seconds % MINUTE, "second"],
          ]
        : [
            [Math.floor(seconds / HOUR), "hour"],
            [Math.floor((seconds % HOUR) / MINUTE), "minute"],
          ];
  return parts
    .filter(([value], index) => index === 0 || value > 0)
    .map(([value, unit]) => formatDuration(value, unit, t.locale))
    .join(" ");
}

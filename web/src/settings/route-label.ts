import { modelName } from "../conversations/model-name";

import type { RouteItem } from "./admin/routes";

/** A route as people read it: its display name, else its model's name as the model picker shows it (WEB-U-111). */
export const routeLabel = (item: RouteItem) => modelName(item);

// Englischer Katalog. Struktur und Signaturen wie im deutschen.
import type { Messages } from "..";
import { common } from "./common";
import { app } from "./app";
import { library } from "./library";
import { storage } from "./storage";
import { events } from "./events";
import { cull } from "./cull";
import { dupes } from "./dupes";
import { reorganize } from "./reorganize";
import { plan } from "./plan";
import { settings } from "./settings";

export const en: Messages = {
  common,
  app,
  library,
  storage,
  events,
  cull,
  dupes,
  reorganize,
  plan,
  settings,
};

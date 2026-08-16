// Solid UI root — composes the HUD chrome, inspector dock, outliner rail
// and search omnibox. Mounted once by main.js with an actions object.

import { render } from "solid-js/web";
import html from "solid-js/html";

import { Hud } from "./hud.js";
import { InspectorDock, HoverTip } from "./inspector.js";
import { Outliner } from "./outliner.js";
import { Search } from "./search.js";

export function mountUI(actions) {
  render(
    () => html`
      ${Hud(actions)}
      ${InspectorDock(actions)}
      ${Outliner(actions)}
      ${Search(actions)}
      ${HoverTip()}`,
    document.getElementById("hud-root"),
  );
}

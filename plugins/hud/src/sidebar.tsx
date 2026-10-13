// The HUD sidebar: usage limits and memory, then one session's context, MCP servers and skills. The plugin's
// main script keeps the state and broadcasts it here.

import { showSidebar } from "wings-plugin-ui";

import { Hud } from "./components/Hud";

showSidebar(<Hud />);

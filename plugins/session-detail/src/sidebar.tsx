// The Session sidebar: what one Claude session is doing now, the files it edited and its recent tool calls.
// The plugin's main script keeps the state and broadcasts it here.

import { showSidebar } from "wings-plugin-ui";

import { Sidebar } from "./components/Sidebar";
import "./sidebar.css";

showSidebar(<Sidebar />);

// The Changes sidebar: the changed files of the focused pane's repo as a folder tree, and its commit history. The
// plugin's main script runs git and broadcasts the files and commits here; a click asks it to open the diff viewer.

import { showSidebar } from "wings-plugin-ui";

import { Sidebar } from "./components/Sidebar";
import "./sidebar.css";

showSidebar(<Sidebar />);

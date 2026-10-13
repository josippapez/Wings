// Points to the Project page, which replaced this plugin.

import { Empty, showSidebar } from "wings-plugin-ui";

function Sidebar() {
  return (
    <Empty
      title="Project info moved to the Project page"
      description="Open it from a project in the sidebar, the + menu or the command palette. Your facts and sections came along. You can turn this plugin off."
    />
  );
}

showSidebar(<Sidebar />);

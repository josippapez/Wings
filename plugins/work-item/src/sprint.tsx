// The Sprint block of a Project page: the work items of the sprint running now in the project's Azure DevOps
// project, a column per state. ui/sprint-view.tsx draws it, so a fixture can render it too.

import { showView } from "wings-plugin-ui";
import { Sprint } from "./ui/sprint-view";

showView(<Sprint />);

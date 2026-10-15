// Work item: shows the Azure Boards work item named in each pane's branch, like `feature/1234-login`, as a pane badge
// with its state, sprint and parent, and moves it to its next state. Each feature has its module in main/.

import { showBadges } from "./main/badge";
import { showSections } from "./main/sections";
import { answerSprint } from "./main/sprint";

showBadges();
showSections();
answerSprint();

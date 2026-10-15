// The list grouped by status, searched: a word that is only in a description or a log note still finds its issue.
import { BoardView } from "../src/lib/board-view";
import "../src/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <BoardView project={project} issues={issues} error={null} showCanceled={false} onShowCanceled={() => {}} layout="list" onLayout={() => {}} byParent={false} folded={[]} onByParent={() => {}} onFold={() => {}} query="rounding totals" onQuery={() => {}} apply={none} send={none} onOpen={() => {}} />;

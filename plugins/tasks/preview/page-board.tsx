// The Issues tab as the page runs it: its issues read from storage, its layout from the Default view setting (the board).
import BoardPage from "../src/pages/board";
import { seedStorage } from "./_data";

export { project } from "./_data";

export const fill = true;
export default function Page() {
  seedStorage();
  return <BoardPage />;
}

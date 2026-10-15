// The Issues block as the page runs it, with its issues read from storage.
import BlockPage from "../src/pages/block";
import { seedStorage } from "./_data";

export { project } from "./_data";

export const place = "view";
export default function Page() {
  seedStorage();
  return <BlockPage />;
}

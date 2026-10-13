// The PR page while its status and conversation load.
import { PrView } from "../src/ui/pr-view";
import "../src/pr.css";
import { props } from "./_data";

export const fill = true;
export default <PrView {...props} overview={null} comments={null} onBack={() => {}} />;

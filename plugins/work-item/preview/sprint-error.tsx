// The Sprint block when Azure DevOps could not be read.
import Sprint from "../src/pages/sprint";

export const place = "view";
export const answers = {
  sprint: (): never => {
    throw new Error("az boards query failed: Please run 'az login' to set up an account.");
  },
};
export default <Sprint />;

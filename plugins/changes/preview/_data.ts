// Made-up data the Changes fixtures share.
import type { CommitRow, FileRow, HistoryMessage, StateMessage } from "../src/lib/types";

export const repo = { root: "/work/acme/web", name: "web", branch: "feature/checkout-redesign" };

const f = (status: string, path: string, added: number | null, removed: number | null, oldPath: string | null = null): FileRow => [status, path, added, removed, oldPath];

export const files: FileRow[] = [
  f("M", "package.json", 2, 1),
  f("M", "pnpm-lock.yaml", 148, 96),
  f("M", "README.md", 12, 3),
  f("A", "src/features/checkout/CheckoutPage.tsx", 184, 0),
  f("A", "src/features/checkout/CheckoutSummary.tsx", 96, 0),
  f("M", "src/features/checkout/index.ts", 4, 1),
  f("D", "src/features/checkout/LegacyCheckout.tsx", 0, 212),
  f("R", "src/features/checkout/hooks/useCart.ts", 18, 6, "src/hooks/useCart.ts"),
  f("M", "src/features/checkout/hooks/usePayment.ts", 41, 17),
  f("M", "src/features/checkout/hooks/useShippingRates.ts", 7, 2),
  f("?", "src/features/checkout/hooks/useCoupon.ts", 24, 0),
  f("M", "src/components/Button/Button.tsx", 9, 4),
  f("M", "src/components/Button/Button.module.css", 22, 8),
  f("M", "src/components/Button/Button.stories.tsx", 31, 0),
  f("A", "src/components/Stepper/Stepper.tsx", 73, 0),
  f("A", "src/components/Stepper/Stepper.module.css", 58, 0),
  f("A", "src/components/Stepper/index.ts", 1, 0),
  f("M", "src/lib/api/client.ts", 14, 9),
  f("M", "src/lib/api/orders.ts", 33, 12),
  f("M", "src/lib/api/payments.ts", 5, 5),
  f("M", "src/lib/format/currency.ts", 11, 2),
  f("U", "src/lib/format/date.ts", 6, 6),
  f("M", "src/routes/checkout.tsx", 27, 40),
  f("M", "src/routes/cart.tsx", 3, 1),
  f("M", "src/styles/tokens.css", 16, 4),
  f("?", "src/assets/icons/card-visa.svg", null, 0),
  f("?", "src/assets/icons/card-mastercard.svg", null, 0),
  f("M", "src/assets/hero@2x.png", null, null),
  f("M", "tests/checkout/checkout.spec.ts", 88, 12),
  f("A", "tests/checkout/coupon.spec.ts", 64, 0),
  f("M", "tests/checkout/fixtures/cart.json", 20, 0),
  f("M", "tests/e2e/smoke.spec.ts", 2, 2),
  f("M", ".github/workflows/ci.yml", 8, 1),
  f("M", "docs/architecture/checkout-flow.md", 52, 14),
  f("A", "docs/architecture/payments.md", 40, 0),
  f("M", "config/feature-flags.json", 3, 0),
  f("M", "scripts/seed-orders.mjs", 19, 7),
  f("M", "src/features/account/AccountMenu.tsx", 4, 4),
  f("M", "src/features/account/hooks/useProfile.ts", 12, 3),
  f("M", "src/features/orders/OrderList.tsx", 26, 18),
];

export const state = (extra: Partial<StateMessage> = {}): StateMessage => ({
  type: "state",
  mode: "uncommitted",
  repo,
  against: "against HEAD (a1b2c3d)",
  files,
  total: files.length,
  added: files.reduce((n, r) => n + (r[2] ?? 0), 0),
  removed: files.reduce((n, r) => n + (r[3] ?? 0), 0),
  ...extra,
});

const now = Math.floor(Date.now() / 1000);
const c = (hash: string, subject: string, author: string, ago: number, refs: string[] = []): CommitRow => [hash, subject, author, now - ago, refs];
export const commits: CommitRow[] = [
  c("a1b2c3d4e5f6", "Add a coupon field to the checkout summary", "Ada Lovelace", 600, ["H:feature/checkout-redesign", "r:origin/feature/checkout-redesign"]),
  c("b2c3d4e5f6a1", "Merge branch 'main' into feature/checkout-redesign", "Ada Lovelace", 5400, []),
  c("c3d4e5f6a1b2", "Merge pull request #482 from acme/fix-rounding", "Grace Hopper", 20_000, ["r:origin/main", "b:main"]),
  c("d4e5f6a1b2c3", "Round shipping rates to the nearest cent before they are summed with the order total", "Alan Turing", 86_400, []),
  c("e5f6a1b2c3d4", "Release 2.14.0", "Grace Hopper", 2 * 86_400, ["t:v2.14.0"]),
  c("f6a1b2c3d4e5", "Replace the legacy checkout page", "Ada Lovelace", 3 * 86_400, ["b:feature/payments-v2", "r:origin/feature/payments-v2", "t:v2.14.0-rc.1"]),
  c("0a1b2c3d4e5f", "Fix", "Linus Torvalds", 4 * 86_400, []),
  c("1b2c3d4e5f6a", "Squash merge of pull request #470: speed up order list", "Margaret Hamilton", 9 * 86_400, ["r:origin/release/2.13", "b:release/2.13"]),
  c("2c3d4e5f6a1b", "Bump the payment SDK to the latest minor", "Grace Hopper", 12 * 86_400, []),
  c("3d4e5f6a1b2c", "Initial commit", "Ada Lovelace", 200 * 86_400, ["t:v0.1.0"]),
];

export const history = (extra: Partial<HistoryMessage> = {}): HistoryMessage => ({ type: "history", query: "", all: false, skip: 0, repo, rows: commits, more: true, ...extra });

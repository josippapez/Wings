import { Component, type ReactNode } from "react";

/** Shows `fallback` instead of its children when one of them throws while rendering, so one bad item can't unmount the app. */
export class ErrorBoundary extends Component<{ children: ReactNode; fallback: ReactNode; /** Resets the boundary when this changes. */ resetKey?: string }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidCatch(error: unknown) {
    console.error(error);
  }
  componentDidUpdate(prev: { resetKey?: string }) {
    if (this.state.failed && prev.resetKey !== this.props.resetKey) this.setState({ failed: false });
  }
  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

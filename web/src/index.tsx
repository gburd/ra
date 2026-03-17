import { render } from "preact";
import { App } from "src/components/App.tsx";
import "src/styles/global.css";

const root = document.getElementById("app");
if (root) {
  render(<App />, root);
}

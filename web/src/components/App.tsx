import Router from "preact-router";
import { useState } from "preact/hooks";
import type { DatabaseId } from "src/types.ts";
import { Header } from "src/components/shared/Header.tsx";
import { EditorPage } from "src/components/editor/EditorPage.tsx";
import { ComparePage } from "src/components/comparison/ComparePage.tsx";
import { IsolationPage } from "src/components/isolation/IsolationPage.tsx";
import { TranslatePage } from "src/components/translation/TranslatePage.tsx";
import { HomePage } from "src/components/shared/HomePage.tsx";
import { SharePage } from "src/components/shared/SharePage.tsx";
import { DemosPage } from "src/components/demonstrations/DemosPage.tsx";

export function App() {
  const [activeDb, setActiveDb] = useState<DatabaseId>("sqlite");

  return (
    <div class="app">
      <Header activeDb={activeDb} onDbChange={setActiveDb} />
      <main class="main-content">
        <Router>
          <HomePage path="/" />
          <EditorPage path="/editor" database={activeDb} />
          <ComparePage path="/compare" />
          <IsolationPage path="/isolation" database={activeDb} />
          <TranslatePage path="/translate" />
          <DemosPage path="/demos" />
          <SharePage path="/share/:id" />
        </Router>
      </main>
    </div>
  );
}

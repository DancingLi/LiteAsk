import { useEffect, useState } from "react";
import { ToolbarWindow } from "./windows/ToolbarWindow";
import { ActionWindow } from "./windows/ActionWindow";
import { QuickAskWindow } from "./windows/QuickAskWindow";
import { SettingsWindow } from "./windows/SettingsWindow";

function getRoute(): string {
  return window.location.hash.replace(/^#/, "") || "/settings";
}

export function App() {
  const [route, setRoute] = useState(getRoute);

  useEffect(() => {
    const onHashChange = () => setRoute(getRoute());
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  switch (route) {
    case "/toolbar":
      return <ToolbarWindow />;
    case "/action":
      return <ActionWindow />;
    case "/quick-ask":
      return <QuickAskWindow />;
    case "/settings":
    default:
      return <SettingsWindow />;
  }
}

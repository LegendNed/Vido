import { getCurrentWindow } from "@tauri-apps/api/window";

import { RegionPicker } from "./features/capture/RegionPicker";
import { EditorWindow } from "./features/editor/EditorWindow";
import { Hud } from "./features/hud/Hud";
import { MainWindow } from "./features/main/MainWindow";

/** Every Vido window loads this bundle; the window label picks the view. */
export function App() {
  const label = getCurrentWindow().label;
  switch (label) {
    case "editor":
      return <EditorWindow />;
    case "hud":
      return <Hud />;
    case "region":
      return <RegionPicker />;
    default:
      return <MainWindow />;
  }
}

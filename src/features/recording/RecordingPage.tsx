import { AudioSettings } from "../audio/AudioPage";
import { CaptureSettings } from "../capture/CapturePage";

/** What the replay buffer records: video source, quality, buffer, then audio. */
export function RecordingPage() {
  return (
    <div className="page">
      <div className="page-head">
        <h1>Recording</h1>
      </div>
      <CaptureSettings />
      <AudioSettings />
    </div>
  );
}

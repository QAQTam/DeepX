import { render } from "solid-js/web";
import App from "./App";
import "./App.css";
import "./styles/markdown.css";
import "./styles/sidebar.css";
import "./styles/chat-view.css";
import "./styles/startup-view.css";
import "./styles/message-list.css";
import "./styles/tool-call-card.css";
import "./styles/input-bar.css";
import "./styles/settings.css";
import "./styles/status-panel.css";
import "./styles/info-bar.css";
import "./styles/ask-dialog.css";
import "./styles/token-chart.css";

render(() => <App />, document.getElementById("root")!);

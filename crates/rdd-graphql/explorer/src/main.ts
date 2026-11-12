import { createApp } from "vue";

import App from "./App.vue";
import { applyTheme } from "./lib/theme";
import "./style.css";

applyTheme();

const root = document.getElementById("app");
if (root !== null) createApp(App).mount(root);

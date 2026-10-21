import { mount } from "svelte";
import "../app.css";
import { blockContextMenu } from "../lib/contextMenu";
import { reportErrors } from "../lib/report";
import App from "./App.svelte";
reportErrors("settings");
blockContextMenu();
export default mount(App, { target: document.getElementById("app")! });

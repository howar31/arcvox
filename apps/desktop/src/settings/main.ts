import { mount } from "svelte";
import "../app.css";
import { blockContextMenu } from "../lib/contextMenu";
import App from "./App.svelte";
blockContextMenu();
export default mount(App, { target: document.getElementById("app")! });

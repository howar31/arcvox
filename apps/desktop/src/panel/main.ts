import { mount } from "svelte";
import "../app.css";
import { blockContextMenu } from "../lib/contextMenu";
import Panel from "./Panel.svelte";
blockContextMenu(true);
export default mount(Panel, { target: document.getElementById("app")! });

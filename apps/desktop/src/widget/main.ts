import { mount } from "svelte";
import "../app.css";
import { blockContextMenu } from "../lib/contextMenu";
import Widget from "./Widget.svelte";
blockContextMenu(true);
export default mount(Widget, { target: document.getElementById("app")! });

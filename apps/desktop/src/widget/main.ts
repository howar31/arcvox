import { mount } from "svelte";
import "../app.css";
import { blockContextMenu } from "../lib/contextMenu";
import { reportErrors } from "../lib/report";
import Widget from "./Widget.svelte";
reportErrors("widget");
blockContextMenu(true);
export default mount(Widget, { target: document.getElementById("app")! });

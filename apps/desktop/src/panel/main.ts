import { mount } from "svelte";
import "../app.css";
import { blockContextMenu } from "../lib/contextMenu";
import { reportErrors } from "../lib/report";
import Panel from "./Panel.svelte";
reportErrors("panel");
blockContextMenu(true);
export default mount(Panel, { target: document.getElementById("app")! });

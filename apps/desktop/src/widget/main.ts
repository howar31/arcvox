import { mount } from "svelte";
import "../app.css";
import Widget from "./Widget.svelte";
export default mount(Widget, { target: document.getElementById("app")! });

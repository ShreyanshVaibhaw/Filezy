import { mount } from "svelte";
import App from "./App.svelte";
import "./styles/fluent.css";
import "./styles/themes.css";

export default mount(App, { target: document.getElementById("app")! });

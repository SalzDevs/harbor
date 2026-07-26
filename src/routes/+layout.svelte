<script lang="ts">
  import { strings } from "$lib/strings";
  import "../app.css";

  let { children } = $props();
</script>

<svelte:head>
  <title>{strings.appTitle}</title>
</svelte:head>

{@render children()}

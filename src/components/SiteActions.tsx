import { api, type Site } from "../lib/api";
import { useModules } from "../lib/modules";
import { openTerminal } from "../lib/terminal";
import { openTab, siteTab } from "../lib/layout";
import { editorName, useCodeEditors } from "../lib/editors";
import { useT } from "../lib/i18n";
import { IconButton } from "./IconButton";

/**
 * A site's buttons, as square icons named in tooltips: Terminal, Open folder,
 * Open in the code editor, Manage (on the Sites list) and Remove. The Sites
 * list and the site's own tab show the same ones.
 */
export function SiteActions({
  site,
  manage = false,
  onRun,
  onRemove,
}: {
  site: Site;
  /** Show Manage, which opens the site in its own tab. */
  manage?: boolean;
  /** Runs an open action, showing what went wrong if it fails. */
  onRun: (open: () => Promise<unknown>) => void;
  onRemove: () => void;
}) {
  const t = useT();
  const { isOn } = useModules();
  const editor = editorName(useCodeEditors());
  return (
    <div className="icon-actions" role="group" aria-label={t("Actions for {site}", { site: site.domain })}>
      {isOn("terminal") && (
        <IconButton icon="terminal" label={t("Terminal")} onClick={() => openTerminal(site.project.folder, site.domain)} />
      )}
      <IconButton icon="folder" label={t("Open folder")} onClick={() => onRun(() => api.openSiteFolder(site.domain))} />
      <IconButton icon="code" label={t("Open in {editor}", { editor })} onClick={() => onRun(() => api.openSiteInEditor(site.domain))} />
      {manage && <IconButton icon="manage" label={t("Manage")} onClick={() => openTab(siteTab(site.domain))} />}
      <IconButton icon="trash" danger label={t("Remove (the files stay)")} onClick={onRemove} />
    </div>
  );
}

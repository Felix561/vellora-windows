type Props<T extends string> = {
  /** Prefix for tab ids (`<prefix>-tab-<id>`) and panel ids (`<prefix>-panel-<id>`). */
  idPrefix: string;
  label: string;
  tabs: ReadonlyArray<{ id: T; label: string }>;
  selected: T;
  onSelect: (id: T) => void;
};

/** WAI-ARIA tab strip with roving focus (arrow keys, Home, End). */
export function SectionTabs<T extends string>({ idPrefix, label, tabs, selected, onSelect }: Props<T>) {
  return (
    <div className="settings-tabs" role="tablist" aria-label={label}>
      {tabs.map((tab, index) => (
        <button key={tab.id} id={`${idPrefix}-tab-${tab.id}`} type="button" role="tab"
          aria-selected={selected === tab.id} aria-controls={`${idPrefix}-panel-${tab.id}`}
          tabIndex={selected === tab.id ? 0 : -1}
          onClick={() => onSelect(tab.id)}
          onKeyDown={(event) => {
            const last = tabs.length - 1;
            const nextIndex = event.key === "ArrowRight" ? (index === last ? 0 : index + 1)
              : event.key === "ArrowLeft" ? (index === 0 ? last : index - 1)
                : event.key === "Home" ? 0
                  : event.key === "End" ? last
                    : null;
            if (nextIndex === null) return;
            event.preventDefault();
            const next = tabs[nextIndex].id;
            onSelect(next);
            document.getElementById(`${idPrefix}-tab-${next}`)?.focus();
          }}>
          {tab.label}
        </button>
      ))}
    </div>
  );
}

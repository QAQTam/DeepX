export type EnvironmentState = {
  changes: { additions: number; deletions: number };
  files: string[];
};

export function environmentFromGit(files: Array<{ additions?: number; deletions?: number; path?: string }>): EnvironmentState {
  return files.reduce((state, file) => ({
    changes: {
      additions: state.changes.additions + (file.additions ?? 0),
      deletions: state.changes.deletions + (file.deletions ?? 0),
    },
    files: file.path ? [...state.files, file.path] : state.files,
  }), { changes: { additions: 0, deletions: 0 }, files: [] } as EnvironmentState);
}

export function applyCodeDelta(state: EnvironmentState, delta: { lines_added: number; lines_removed: number }): EnvironmentState {
  return { ...state, changes: {
    additions: state.changes.additions + delta.lines_added,
    deletions: state.changes.deletions + delta.lines_removed,
  }};
}

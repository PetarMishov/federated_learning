import { DatePipe, DecimalPipe } from '@angular/common';
import { HttpErrorResponse, HttpEventType } from '@angular/common/http';
import { Component, DestroyRef, OnInit, inject, signal, computed, HostListener } from '@angular/core';
import { takeUntilDestroyed, toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router } from '@angular/router';
import { BehaviorSubject, EMPTY, Subject, Subscription, from, timer, expand, filter, take, catchError, finalize, startWith, switchMap, tap, skip, takeUntil, of } from 'rxjs';
import { folderFiles } from './folder-files';
import { BranchPicker } from '../branch-picker/branch-picker';
import { RepositoryPicker } from '../repository-picker/repository-picker';
import { Branch, Repository, Deployment, Member, Snapshot, SnapshotFile, SnapshotPreviewTooLarge, SnapshotTreeEntry, SnapshotOperation, ImportStatus, UsersApi } from '../users-api';

type EditorSource =
  | { kind: 'snapshot'; id: number; project_id: number }
  | { kind: 'draft'; id: string; project_id: number };

@Component({
  selector: 'app-project',
  imports: [DatePipe, DecimalPipe, RepositoryPicker, BranchPicker],
  templateUrl: './project.html',
  styleUrl: './project.css',
})
export class ProjectPage implements OnInit {
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  private readonly api = inject(UsersApi);
  private readonly destroyRef = inject(DestroyRef);
  private readonly retryDeployments = new Subject<void>();
  private readonly retryMembers = new Subject<void>();
  private readonly snapshotSelection = new Subject<number | null>();
  private readonly loadedSnapshot = new BehaviorSubject<EditorSource | null>(null);
  private readonly directorySelection = new Subject<string>();
  private readonly fileSelection = new Subject<string | null>();
  protected readonly directoryPath = signal('');
  protected readonly treeLoading = signal(false);
  protected readonly treeError = signal('');
  protected readonly selectedFilePath = signal<string | null>(null);
  protected readonly file = signal<SnapshotFile | null>(null);
  protected readonly fileLoading = signal(false);
  protected readonly fileError = signal('');
  protected readonly filePreviewTooLarge = signal<SnapshotPreviewTooLarge | null>(null);
  private readonly retrySnapshots = new Subject<number>();
  private autoSelectSnapshot = true;
  private pendingOpenFile: { snapshotId: number; path: string } | null = null;
  private readonly drafts = signal(new Map<number | string, Map<string, { original: string | null; content: string }>>());
  private readonly draftFolders = signal(new Map<number | string, Set<string>>());
  protected readonly creationKind = signal<'file' | 'folder' | 'rename' | null>(null);
  protected readonly creationError = signal('');
  private readonly directoryCache = signal(new Map<number | string, Map<string, SnapshotTreeEntry[]>>());
  protected readonly expandedFolders = signal(new Set<string>());
  private readonly operations = signal(new Map<number | string, SnapshotOperation[]>());
  protected readonly structuralChanges = computed(() => this.operations().get(this.editorSource()?.id ?? -1) ?? []);
  protected readonly movingEntry = signal(false);
  protected readonly contextMenu = signal<{ entry: SnapshotTreeEntry | null; x: number; y: number } | null>(null);
  private menuFocusTimeout: ReturnType<typeof setTimeout> | undefined;
  private menuTrigger: HTMLElement | null = null;
  private renameTarget: SnapshotTreeEntry | null = null;
  private draggedPath: string | null = null;
  protected readonly treeRows = computed(() => {
    const rows: (SnapshotTreeEntry & { depth: number })[] = [];
    const visit = (parent: string, depth: number) => {
      for (const entry of this.children(parent)) {
        rows.push({ ...entry, depth });
        if (entry.kind === 'directory' && this.expandedFolders().has(entry.path)) visit(entry.path, depth + 1);
      }
    };
    visit('', 0);
    return rows;
  });

  private children(parent: string): SnapshotTreeEntry[] {
    const id = this.editorSource()?.id;
    if (id === undefined) return [];
    const entries = new Map((this.directoryCache().get(id)?.get(parent) ?? []).map((entry) => [entry.path, entry]));
    const add = (path: string, kind: 'file' | 'directory') => {
      if (this.parentPath(path) !== parent) return;
      entries.set(path, { name: path.split('/').pop()!, path, kind });
    };
    for (const path of this.draftFolders().get(id) ?? []) add(path, 'directory');
    for (const [path, draft] of this.drafts().get(id) ?? []) if (draft.original === null) add(path, 'file');
    return [...entries.values()].sort((a, b) => Number(b.kind === 'directory') - Number(a.kind === 'directory') || a.name.localeCompare(b.name));
  }
  protected readonly savingSnapshot = signal(false);
  protected readonly saveError = signal('');
  protected readonly saveMessage = signal('');
  protected readonly changedFiles = computed(() => {
    const id = this.editorSource()?.id;
    return id === undefined ? [] : [...(this.drafts().get(id)?.entries() ?? [])]
      .filter(([, draft]) => draft.content !== draft.original)
      .map(([path, draft]) => ({ path, content: draft.content }));
  });
  protected readonly selectedSnapshotId = signal<number | null>(null);
  protected readonly snapshots = signal<Snapshot[]>([]);
  protected readonly snapshotsLoading = signal(false);
  protected readonly snapshotsError = signal('');
  protected readonly snapshotsHasMore = signal(false);
  protected readonly snapshot = signal<Snapshot | null>(null);
  protected readonly snapshotLoading = signal(false);
  protected readonly snapshotError = signal('');
  protected readonly commitCopyMessage = signal('');
  private commitCopyTimeout: ReturnType<typeof setTimeout> | undefined;
  protected readonly members = signal<Member[]>([]);
  protected readonly membersLoading = signal(false);
  protected readonly membersError = signal('');
  protected readonly deployments = signal<Deployment[]>([]);
  protected readonly deploymentsLoading = signal(false);
  protected readonly deploymentsError = signal('');
  protected readonly source = signal<'github' | 'gitlab' | 'local'>('github');
  protected readonly selectedRepository = signal<Repository | null>(null);
  protected readonly selectedBranch = signal<Branch | null>(null);
  protected readonly sourceCommit = signal('');
  protected readonly editorSource = signal<EditorSource | null>(null);
  protected readonly importDraft = signal<ImportStatus | null>(null);
  protected readonly importLoading = signal(false);
  protected readonly importError = signal('');
  protected readonly importPhase = signal<ImportStatus['phase']>('preparing');
  protected readonly importProgress = signal<number | null>(null);
  private importRequest: Subscription | undefined;
  private projectId = 0;
  private activeImport: { projectId: number; id: string } | null = null;
  protected readonly localPath = signal('');
  protected readonly localFiles = signal<File[]>([]);
  protected readonly repositoryProvider = computed(() => this.source() === 'gitlab' ? 'gitlab' : 'github');
  protected readonly drawer = signal<'members' | 'deployments' | null>(null);
  protected readonly queryParams = toSignal(this.route.queryParamMap, {
    initialValue: this.route.snapshot.queryParamMap,
  });

  constructor() {
    this.destroyRef.onDestroy(() => {
      clearTimeout(this.commitCopyTimeout);
      clearTimeout(this.menuFocusTimeout);
      this.cancelImport();
      this.discardDraft();
    });
  }

  ngOnInit() {
    this.loadedSnapshot.pipe(
      switchMap((saved) => {
        this.directoryPath.set('');
        this.expandedFolders.set(new Set());
        this.contextMenu.set(null);
        this.treeLoading.set(false);
        this.treeError.set('');
        if (!saved) return EMPTY;
        return this.directorySelection.pipe(
          startWith(''),
          switchMap((path) => {
            this.creationKind.set(null);
            this.creationError.set('');
            this.directoryPath.set(path);
            this.treeError.set('');
            this.treeLoading.set(true);
            if (path) this.expandPath(path);
            const original = this.originalPath(path);
            const tree = this.draftFolders().get(saved.id)?.has(path) || original === null
              ? of({ entries: [] as SnapshotTreeEntry[] })
              : this.readTree(saved, original);
            return tree.pipe(
              tap((tree) => {
                const entries = tree.entries.map((entry) => this.transformEntry(entry)).filter((entry): entry is SnapshotTreeEntry => entry !== null);
                this.cacheDirectory(saved.id, path, entries);
              }),
              catchError((error: HttpErrorResponse) => {
                this.handleFileAccessError(error);
                this.treeError.set(error.status === 404
                  ? 'Folder not found or you no longer have access.'
                  : 'Could not load files. Please try again.');
                return EMPTY;
              }),
              finalize(() => this.treeLoading.set(false)),
            );
          }),
        );
      }),
      takeUntilDestroyed(this.destroyRef),
    ).subscribe();

    this.loadedSnapshot.pipe(
      switchMap((saved) => {
        this.selectedFilePath.set(null);
        this.file.set(null);
        this.fileError.set('');
        this.filePreviewTooLarge.set(null);
        this.fileLoading.set(false);
        if (!saved) return EMPTY;
        return this.fileSelection.pipe(
          startWith(null),
          switchMap((path) => {
            this.selectedFilePath.set(path);
            this.file.set(null);
            this.fileError.set('');
            this.filePreviewTooLarge.set(null);
            this.fileLoading.set(false);
            if (path === null) return EMPTY;
            this.fileLoading.set(true);
            const draft = this.drafts().get(saved.id)?.get(path);
            const request = draft?.original === null ? of({ path, content: draft.content })
              : this.readFile(saved, this.originalPath(path) ?? path);
            return request.pipe(
              tap((response) => {
                const file = { ...response, path };
                const drafts = new Map(this.drafts());
                const files = new Map(drafts.get(saved.id));
                if (!files.has(file.path)) files.set(file.path, { original: file.content, content: file.content });
                drafts.set(saved.id, files);
                this.drafts.set(drafts);
                this.file.set({ ...file, content: files.get(file.path)!.content });
              }),
              catchError((error: HttpErrorResponse) => {
                this.handleFileAccessError(error);
                const details = error.error;
                if (error.status === 413 && details?.error === 'preview_too_large'
                  && Number.isSafeInteger(details.size_bytes) && details.size_bytes >= 0
                  && Number.isSafeInteger(details.max_preview_bytes) && details.max_preview_bytes > 0) {
                  this.filePreviewTooLarge.set(details);
                  return EMPTY;
                }
                this.fileError.set(error.status === 415
                  ? 'This entry cannot be displayed as a text file.'
                  : error.status === 413 ? 'This file is too large to preview.'
                  : error.status === 404 ? 'File not found or you no longer have access.'
                  : 'Could not load the file. Please try again.');
                return EMPTY;
              }),
              finalize(() => this.fileLoading.set(false)),
            );
          }),
        );
      }),
      takeUntilDestroyed(this.destroyRef),
    ).subscribe();

    this.route.paramMap.pipe(
      switchMap((params) => {
        this.projectId = Number(params.get('id'));
        this.cancelImport();
        this.discardDraft();
        this.pendingOpenFile = null;
        this.operations.set(new Map());
        this.directoryCache.set(new Map());
        this.draftFolders.set(new Map());
        this.drafts.set(new Map());
        this.saveError.set('');
        this.saveMessage.set('');
        this.selectedSnapshotId.set(null);
        return this.snapshotSelection.pipe(
          startWith(null),
          switchMap((snapshotId) => {
            this.setEditor(null);
            this.snapshot.set(null);
            this.clearCommitCopyMessage();
            this.snapshotError.set('');
            this.snapshotLoading.set(false);
            if (snapshotId === null) return EMPTY;
            this.snapshotLoading.set(true);
            return this.api.getSnapshot(params.get('id') ?? '', snapshotId).pipe(
              tap((snapshot) => {
                this.snapshot.set(snapshot);
                this.setEditor({ kind: 'snapshot', id: snapshot.id, project_id: snapshot.project_id });
                if (this.pendingOpenFile?.snapshotId === snapshot.id) {
                  this.fileSelection.next(this.pendingOpenFile.path);
                  this.pendingOpenFile = null;
                }
              }),
              catchError((error: HttpErrorResponse) => {
                if (error.status === 401) {
                  this.api.clearSession();
                  void this.router.navigateByUrl('/login');
                } else {
                  this.snapshotError.set(error.status === 404
                    ? 'Snapshot not found or you no longer have access.'
                    : 'Could not load the snapshot. Please try again.');
                }
                return EMPTY;
              }),
              finalize(() => this.snapshotLoading.set(false)),
            );
          }),
        );
      }),
      takeUntilDestroyed(this.destroyRef),
    ).subscribe();

    this.route.paramMap.pipe(
      switchMap((params) => {
        this.autoSelectSnapshot = true;
        this.snapshots.set([]);
        this.snapshotsHasMore.set(false);
        return this.retrySnapshots.pipe(
          startWith(0),
          switchMap((offset) => {
            this.snapshotsLoading.set(true);
            this.snapshotsError.set('');
            return this.api.getSnapshots(params.get('id') ?? '', offset).pipe(
              tap((result) => {
                this.snapshots.set(offset === 0 ? result.snapshots : [...this.snapshots(), ...result.snapshots]);
                this.snapshotsHasMore.set(result.has_more);
                if (this.autoSelectSnapshot && result.snapshots.length) {
                  this.selectSnapshot(result.snapshots[0].id);
                }
              }),
              catchError((error: HttpErrorResponse) => {
                this.handleFileAccessError(error);
                this.snapshotsError.set(error.status === 404
                  ? 'Project not found or you no longer have access.'
                  : 'Could not load snapshots. Please try again.');
                return EMPTY;
              }),
              finalize(() => this.snapshotsLoading.set(false)),
            );
          }),
        );
      }),
      takeUntilDestroyed(this.destroyRef),
    ).subscribe();

    this.route.paramMap.pipe(
      switchMap((params) => this.retryMembers.pipe(
        startWith(undefined),
        switchMap(() => {
          this.members.set([]);
          this.membersError.set('');
          this.membersLoading.set(false);
          if (this.drawer() !== 'members') return EMPTY;
          this.membersLoading.set(true);
          return this.api.getProjectMembers(params.get('id') ?? '').pipe(
            tap((result) => this.members.set(result.members)),
            catchError((error: HttpErrorResponse) => {
              if (error.status === 401) {
                this.api.clearSession();
                void this.router.navigateByUrl('/login');
              } else {
                this.membersError.set(error.status === 404
                  ? 'Project not found.'
                  : 'Could not load members. Please try again.');
              }
              return EMPTY;
            }),
            finalize(() => this.membersLoading.set(false)),
          );
        }),
      )),
      takeUntilDestroyed(this.destroyRef),
    ).subscribe();

    this.route.paramMap.pipe(
      switchMap((params) => this.retryDeployments.pipe(
        startWith(undefined),
        switchMap(() => {
          this.deployments.set([]);
          this.deploymentsError.set('');
          this.deploymentsLoading.set(true);
          return this.api.getProjectDeployments(params.get('id') ?? '').pipe(
            tap((result) => this.deployments.set(result.deployments)),
            catchError((error: HttpErrorResponse) => {
              if (error.status === 401) {
                this.api.clearSession();
                void this.router.navigateByUrl('/login');
              } else {
                this.deploymentsError.set(error.status === 404
                  ? 'Project not found.'
                  : 'Could not load deployments. Please try again.');
              }
              return EMPTY;
            }),
            finalize(() => this.deploymentsLoading.set(false)),
          );
        }),
      )),
      takeUntilDestroyed(this.destroyRef),
    ).subscribe();
  }

  private unsavedWork() {
    return this.importDraft() !== null
      || [...this.drafts().values()].some(files => [...files.values()].some(file => file.content !== file.original))
      || [...this.operations().values()].some(operations => operations.length > 0);
  }

  canLeave() {
    return (!this.unsavedWork() && !this.importLoading()) || window.confirm('Discard unsaved work and leave this project?');
  }

  @HostListener('window:beforeunload', ['$event'])
  protected beforeUnload(event: BeforeUnloadEvent) {
    if (this.unsavedWork() || this.importLoading()) { event.preventDefault(); event.returnValue = ''; }
  }

  @HostListener('window:pagehide')
  protected releaseImports() {
    if (this.activeImport) this.api.discardImportOnExit(this.activeImport.projectId, this.activeImport.id);
    const source = this.editorSource();
    if (source?.kind === 'draft') this.api.discardImportOnExit(source.project_id, source.id);
    this.importRequest?.unsubscribe();
    this.activeImport = null;
    this.importDraft.set(null);
    this.importLoading.set(false);
    this.drafts.set(new Map());
    this.operations.set(new Map());
    this.draftFolders.set(new Map());
    this.setEditor(null);
  }

  @HostListener('window:pageshow', ['$event'])
  protected restorePage(event: PageTransitionEvent) {
    if (event.persisted) this.snapshotSelection.next(this.selectedSnapshotId());
  }

  private discardDraft() {
    const draft = this.importDraft();
    const source = this.editorSource();
    if (draft && source?.kind === 'draft') this.api.discardImport(source.project_id, draft.id).subscribe({ error: () => {} });
    this.importDraft.set(null);
  }

  protected cancelImport() {
    this.importRequest?.unsubscribe();
    this.importRequest = undefined;
    if (this.activeImport) this.api.discardImport(this.activeImport.projectId, this.activeImport.id).subscribe({ error: () => {} });
    this.activeImport = null;
    this.importLoading.set(false);
  }

  private pollImport(project: number, initial: ImportStatus) {
    return of(initial).pipe(
      expand(status => ['ready', 'failed', 'cancelled'].includes(status.phase) ? EMPTY
        : timer(400).pipe(switchMap(() => this.api.getImport(project, status.id)))),
      tap(status => { this.importPhase.set(status.phase); this.importProgress.set(status.progress_percent); }),
      filter(status => ['ready', 'failed', 'cancelled'].includes(status.phase)), take(1),
    );
  }

  protected loadProject() {
    if (this.importLoading() || this.savingSnapshot()) return;
    const source = this.source();
    const repository = this.selectedRepository();
    const branch = this.selectedBranch();
    const commit = this.sourceCommit().trim();
    this.importError.set('');
    if (source === 'local' && !this.localFiles().length) { this.importError.set('Choose a folder to load.'); return; }
    if (source !== 'local' && (!repository || !branch || !/^[a-fA-F0-9]{40}$/.test(commit))) {
      this.importError.set('Select a repository, branch, and full commit SHA.'); return;
    }
    if (this.unsavedWork() && !window.confirm('Replace this draft and discard its unsaved edits after the new project loads?')) return;
    const previousEdits = JSON.stringify([this.changedFiles(), this.structuralChanges()]);
    const project = this.projectId;
    if (!Number.isSafeInteger(project) || project <= 0) { this.importError.set('Project not found.'); return; }
    this.autoSelectSnapshot = false;
    this.importLoading.set(true);
    this.importPhase.set('preparing'); this.importProgress.set(null);
    const selectedFiles = this.localFiles();
    this.importRequest = this.api.getImportLimits(project).pipe(
      switchMap(limits => source === 'local' ? from(folderFiles(selectedFiles, limits)) : of(null)),
      switchMap(files => this.api.createImport(project, source,
        source === 'local' ? undefined : source === 'github' ? repository!.full_name : String(repository!.id),
        source === 'local' ? undefined : branch!.name, source === 'local' ? undefined : commit,
      ).pipe(
        tap(status => { this.activeImport = { projectId: project, id: status.id }; this.importPhase.set(status.phase); }),
        switchMap(status => files === null ? this.pollImport(project, status) : this.api.uploadImport(project, status.id, files).pipe(
          tap(event => {
            if (event.type === HttpEventType.UploadProgress) {
              this.importPhase.set('uploading'); this.importProgress.set(event.total ? Math.round(event.loaded * 100 / event.total) : null);
            }
          }),
          filter(event => event.type === HttpEventType.Response),
          switchMap(event => event.body ? this.pollImport(project, event.body) : EMPTY),
        )),
      )),
      takeUntil(this.route.paramMap.pipe(skip(1))), takeUntilDestroyed(this.destroyRef),
      finalize(() => this.importLoading.set(false)),
    ).subscribe({
      next: status => {
        if (status.phase !== 'ready') { this.importError.set(status.error || 'Project loading was cancelled.'); this.cancelImport(); return; }
        if (previousEdits !== JSON.stringify([this.changedFiles(), this.structuralChanges()])
          && !window.confirm('You edited files while loading. Discard those edits and open the new project?')) { this.cancelImport(); return; }
        this.discardDraft();
        this.drafts.set(new Map());
        this.draftFolders.set(new Map());
        this.operations.set(new Map());
        this.directoryCache.set(new Map());
        this.activeImport = null;
        this.snapshotSelection.next(null);
        this.pendingOpenFile = null;
        this.selectedSnapshotId.set(null);
        this.importDraft.set(status);
        this.saveError.set(''); this.saveMessage.set('');
        this.setEditor({ kind: 'draft', id: status.id, project_id: project });
      },
      error: (error: unknown) => {
        if (error instanceof HttpErrorResponse) {
          this.handleFileAccessError(error);
          this.importError.set(typeof error.error === 'string' && error.error ? error.error : 'Could not load project. Please try again.');
        } else this.importError.set(error instanceof Error ? error.message : 'Could not load project.');
        this.cancelImport();
      },
    });
  }

  protected selectSource(source: 'github' | 'gitlab' | 'local') {
    if (this.source() === source) return;
    this.source.set(source);
    this.selectRepository(null);
  }

  protected selectLocalFolder(input: HTMLInputElement) {
    const files = Array.from(input.files ?? []);
    // Cancelling the dialog leaves the current path and folder selection intact.
    if (!files.length) return;
    const name = files[0].webkitRelativePath.split('/')[0];
    if (!name) return;
    this.localFiles.set(files);
    this.localPath.set(name);
    // Allow choosing the same folder again after a previous selection.
    input.value = '';
  }

  protected selectRepository(repository: Repository | null) {
    this.selectedRepository.set(repository);
    this.selectBranch(null);
  }

  protected selectBranch(branch: Branch | null) {
    this.selectedBranch.set(branch);
    this.sourceCommit.set(branch?.commit_sha ?? '');
  }

  protected loadDeployments() {
    if (!this.deploymentsLoading()) this.retryDeployments.next();
  }

  protected viewSnapshot(deployment: Deployment) {
    this.selectSnapshot(deployment.snapshot_id);
  }

  protected loadSnapshot() {
    const snapshotId = this.selectedSnapshotId();
    if (snapshotId !== null && !this.snapshotLoading()) this.snapshotSelection.next(snapshotId);
  }

  protected async copySavedCommit() {
    const saved = this.snapshot();
    if (!saved) return;
    this.clearCommitCopyMessage();
    try {
      await navigator.clipboard.writeText(saved.git_commit_sha);
      if (this.snapshot() === saved && !this.destroyRef.destroyed) {
        clearTimeout(this.commitCopyTimeout);
        this.commitCopyMessage.set('Copied!');
        this.commitCopyTimeout = setTimeout(() => this.commitCopyMessage.set(''), 2000);
      }
    } catch {
      if (this.snapshot() === saved && !this.destroyRef.destroyed) {
        this.commitCopyMessage.set('Could not copy. Please try again.');
      }
    }
  }

  private setEditor(source: EditorSource | null) {
    this.editorSource.set(source);
    this.loadedSnapshot.next(source);
  }

  private readTree(source: EditorSource, path: string) {
    return source.kind === 'draft' ? this.api.getImportTree(source.project_id, source.id, path)
      : this.api.getSnapshotTree(source.project_id, source.id, path);
  }

  private readFile(source: EditorSource, path: string) {
    return source.kind === 'draft' ? this.api.getImportFile(source.project_id, source.id, path)
      : this.api.getSnapshotFile(source.project_id, source.id, path);
  }

  private clearCommitCopyMessage() {
    clearTimeout(this.commitCopyTimeout);
    this.commitCopyMessage.set('');
  }

  protected beginCreation(kind: 'file' | 'folder') {
    if (this.savingSnapshot() || this.movingEntry()) return;
    this.contextMenu.set(null);
    this.creationKind.set(kind);
    this.creationError.set('');
  }

  protected createEntry(name: string) {
    const id = this.editorSource()?.id;
    const kind = this.creationKind();
    if (id === undefined || !kind || this.treeLoading() || this.treeError() || this.savingSnapshot() || this.movingEntry()) return;
    const bytes = new TextEncoder();
    if (!name.trim() || name === '.' || name === '..' || /[/\\:\x00-\x1f\x7f-\x9f]/.test(name)
      || name.replace(/[. ]+$/, '').toLowerCase() === '.git' || bytes.encode(name).length > 255) {
      this.creationError.set('Enter a valid name without slashes, control characters or .git.');
      return;
    }
    if (kind === 'rename' && this.renameTarget) {
      const target = this.parentPath(this.renameTarget.path);
      this.moveEntry(this.renameTarget, target ? target + '/' + name : name);
      return;
    }
    if (this.children(this.directoryPath()).some((entry) => entry.name === name)) {
      this.creationError.set('An entry with that name already exists.');
      return;
    }
    const path = this.directoryPath() ? this.directoryPath() + '/' + name : name;
    if (bytes.encode(path).length > 4096) {
      this.creationError.set('This path is too long.');
      return;
    }
    if (kind === 'folder') {
      const folders = new Map(this.draftFolders());
      const paths = new Set(folders.get(id));
      paths.add(path);
      folders.set(id, paths);
      this.draftFolders.set(folders);
    } else {
      const drafts = new Map(this.drafts());
      const files = new Map(drafts.get(id));
      files.set(path, { original: null, content: '' });
      drafts.set(id, files);
      this.drafts.set(drafts);
      this.fileSelection.next(path);
    }
    this.creationKind.set(null);
    this.creationError.set('');
    this.saveError.set('');
    this.saveMessage.set('');
  }

  protected editFile(content: string) {
    const saved = this.editorSource();
    const file = this.file();
    if (!saved || !file || this.savingSnapshot() || this.movingEntry()) return;
    const drafts = new Map(this.drafts());
    const files = new Map(drafts.get(saved.id));
    const original = files.has(file.path) ? files.get(file.path)!.original : file.content;
    // Textareas normalize CRLF; keep a Windows file's original line endings.
    if (original !== null && original.includes('\r\n') && !original.replaceAll('\r\n', '').includes('\n')) {
      content = content.replace(/\r?\n/g, '\r\n');
    }
    files.set(file.path, { original, content });
    drafts.set(saved.id, files);
    this.drafts.set(drafts);
    this.file.set({ ...file, content });
    this.saveError.set('');
    this.saveMessage.set('');
  }

  protected saveSnapshot() {
    const saved = this.editorSource();
    const files = this.changedFiles();
    if (!saved || (saved.kind === 'snapshot' && !files.length && !this.structuralChanges().length) || this.savingSnapshot() || this.movingEntry() || this.importLoading()) return;
    const openedPath = this.selectedFilePath();
    this.savingSnapshot.set(true);
    this.saveError.set('');
    this.saveMessage.set('');
    const save = saved.kind === 'draft'
      ? this.api.saveImportSnapshot(saved.project_id, saved.id, files, this.structuralChanges())
      : this.api.saveSnapshot(saved.project_id, saved.id, files, this.structuralChanges());
    save.pipe(
      takeUntil(this.route.paramMap.pipe(skip(1))),
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.savingSnapshot.set(false)),
    ).subscribe({
      next: (created) => {
        const drafts = new Map(this.drafts());
        const cache = new Map(this.directoryCache());
        cache.delete(saved.id);
        this.directoryCache.set(cache);
        drafts.delete(saved.id);
        this.drafts.set(drafts);
        const folders = new Map(this.draftFolders());
        // Keep empty folders in the new draft; Git records only folders containing files.
        const emptyFolders = new Set([...(folders.get(saved.id) ?? [])]
          .filter((folder) => !files.some((file) => file.path.startsWith(folder + '/'))
            && this.isNewEntry({ name: folder.split('/').pop()!, path: folder, kind: 'directory' })));
        folders.delete(saved.id);
        if (emptyFolders.size) folders.set(created.id, emptyFolders);
        this.draftFolders.set(folders);
        const operations = new Map(this.operations());
        operations.delete(saved.id);
        this.operations.set(operations);
        this.snapshots.update((snapshots) => [created, ...snapshots]);
        this.savingSnapshot.set(false);
        if (openedPath !== null) this.pendingOpenFile = { snapshotId: created.id, path: openedPath };
        this.importDraft.set(null);
        this.selectSnapshot(created.id);
        this.saveMessage.set('Snapshot saved.');
      },
      error: (error: HttpErrorResponse) => {
        this.handleFileAccessError(error);
        this.saveError.set(error.status === 413 ? 'Changes exceed the upload limit.'
          : error.status === 404 ? 'Could not save. The project or editing access is no longer available.'
          : 'Could not save snapshot. Your edits are kept; please try again.');
      },
    });
  }

  protected selectSnapshot(snapshotId: number | null) {
    if (this.savingSnapshot() || this.movingEntry()) return;
    if (this.importDraft() && !window.confirm('Discard the unsaved imported draft and open this snapshot?')) return;
    this.discardDraft();
    this.creationKind.set(null);
    this.creationError.set('');
    this.saveError.set('');
    this.saveMessage.set('');
    if (this.pendingOpenFile?.snapshotId !== snapshotId) this.pendingOpenFile = null;
    this.autoSelectSnapshot = false;
    this.selectedSnapshotId.set(snapshotId);
    this.snapshotSelection.next(snapshotId);
  }

  protected selectedSnapshotIsListed() {
    return this.snapshots().some((saved) => saved.id === this.selectedSnapshotId());
  }

  protected loadSnapshots() {
    if (!this.snapshotsLoading()) this.retrySnapshots.next(0);
  }

  protected loadOlderSnapshots() {
    if (!this.snapshotsLoading()) this.retrySnapshots.next(this.snapshots().length);
  }

  protected openEntry(entry: SnapshotTreeEntry) {
    if (entry.kind === 'directory') {
      if (this.expandedFolders().has(entry.path)) {
        const expanded = new Set(this.expandedFolders());
        expanded.delete(entry.path);
        this.expandedFolders.set(expanded);
        this.directoryPath.set(this.parentPath(entry.path));
      } else this.directorySelection.next(entry.path);
    }
    else if (entry.kind === 'file') this.fileSelection.next(entry.path);
  }

  protected loadTree() {
    if (!this.treeLoading()) this.directorySelection.next(this.directoryPath());
  }

  protected loadFile() {
    const path = this.selectedFilePath();
    if (path !== null && !this.fileLoading()) this.fileSelection.next(path);
  }

  private parentPath(path: string) { return path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : ''; }
  private contains(parent: string, path: string) { return path === parent || path.startsWith(parent + '/'); }
  private expandPath(path: string) {
    const expanded = new Set(this.expandedFolders());
    let part = path;
    while (part) { expanded.add(part); part = this.parentPath(part); }
    this.expandedFolders.set(expanded);
  }
  private cacheDirectory(id: number | string, path: string, entries: SnapshotTreeEntry[]) {
    const cache = new Map(this.directoryCache());
    const directories = new Map(cache.get(id));
    const combined = new Map((directories.get(path) ?? []).map((entry) => [entry.path, entry]));
    for (const entry of entries) if (this.parentPath(entry.path) === path) combined.set(entry.path, entry);
    directories.set(path, [...combined.values()]);
    cache.set(id, directories);
    this.directoryCache.set(cache);
  }
  private originalPath(path: string): string | null {
    for (const operation of [...this.structuralChanges()].reverse()) {
      if (operation.kind === 'move' && this.contains(operation.to, path)) path = operation.path + path.slice(operation.to.length);
      else if (this.contains(operation.path, path)) return null;
    }
    return path;
  }
  private transformEntry(entry: SnapshotTreeEntry): SnapshotTreeEntry | null {
    let path = entry.path;
    for (const operation of this.structuralChanges()) {
      if (!this.contains(operation.path, path)) continue;
      if (operation.kind === 'delete') return null;
      path = operation.to + path.slice(operation.path.length);
    }
    return { ...entry, path, name: path.split('/').pop()! };
  }

  protected contextEntryName() { return this.renameTarget?.name ?? ''; }
  protected keyboardMenu(event: Event, entry: SnapshotTreeEntry) {
    event.preventDefault(); event.stopPropagation();
    const rect = (event.target as HTMLElement).getBoundingClientRect();
    this.showContextMenu(new MouseEvent('contextmenu', { clientX: rect.left, clientY: rect.bottom }), entry);
  }
  protected showContextMenu(event: MouseEvent, entry: SnapshotTreeEntry | null = null) {
    event.preventDefault();
    event.stopPropagation();
    if (!this.editorSource() || this.savingSnapshot() || this.movingEntry() || this.treeLoading()) return;
    this.menuTrigger = event.target instanceof HTMLElement ? event.target : null;
    this.contextMenu.set({ entry, x: Math.min(event.clientX, Math.max(0, window.innerWidth - 180)),
      y: Math.min(event.clientY, Math.max(0, window.innerHeight - 190)) });
    clearTimeout(this.menuFocusTimeout);
    this.menuFocusTimeout = setTimeout(() => {
      if (this.contextMenu()) document.querySelector<HTMLButtonElement>('.file-context-menu button')?.focus();
    }, 0);
  }
  @HostListener('document:click')
  protected dismissMenu() { this.contextMenu.set(null); }
  @HostListener('document:keydown.escape')
  protected cancelFileAction() {
    if (this.contextMenu()) this.menuTrigger?.focus();
    this.contextMenu.set(null); this.creationKind.set(null);
  }
  protected menuKey(event: KeyboardEvent) {
    const buttons = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('button')];
    const index = buttons.indexOf(event.target as HTMLButtonElement);
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      buttons[(index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length]?.focus();
    }
  }

  protected contextCreate(kind: 'file' | 'folder') {
    const entry = this.contextMenu()?.entry;
    const parent = entry?.kind === 'directory' ? entry.path : entry ? this.parentPath(entry.path) : '';
    this.contextMenu.set(null);
    this.directorySelection.next(parent);
    this.beginCreation(kind);
  }
  protected beginRename() {
    const entry = this.contextMenu()?.entry;
    if (!entry) return;
    this.renameTarget = entry;
    this.contextMenu.set(null);
    this.creationError.set('');
    this.creationKind.set('rename');
  }
  protected deleteEntry() {
    const entry = this.contextMenu()?.entry;
    const id = this.editorSource()?.id;
    if (!entry || id === undefined || this.savingSnapshot() || this.movingEntry()) return;
    this.contextMenu.set(null);
    if (!this.isNewEntry(entry)) this.appendOperation({ kind: 'delete', path: entry.path });
    this.rewriteDraftTree(entry, null);
    this.creationKind.set(null);
  }
  private isNewEntry(entry: SnapshotTreeEntry) {
    const id = this.editorSource()!.id;
    if (entry.kind !== 'directory') return this.drafts().get(id)?.get(entry.path)?.original === null;
    if (!this.draftFolders().get(id)?.has(entry.path)) return false;
    let movedRoots: string[] = [];
    for (const operation of this.structuralChanges()) {
      movedRoots = movedRoots.flatMap((path) => this.contains(operation.path, path)
        ? operation.kind === 'delete' ? [] : [operation.to + path.slice(operation.path.length)] : [path]);
      if (operation.kind === 'move') movedRoots.push(operation.to);
    }
    return !movedRoots.some((path) => this.contains(entry.path, path));
  }
  private appendOperation(operation: SnapshotOperation) {
    const operations = new Map(this.operations());
    const id = this.editorSource()!.id;
    operations.set(id, [...(operations.get(id) ?? []), operation]);
    this.operations.set(operations);
    this.saveError.set('');
    this.saveMessage.set('');
  }
  private rewriteDraftTree(entry: SnapshotTreeEntry, destination: string | null) {
    const id = this.editorSource()!.id;
    const rewrite = (path: string) => this.contains(entry.path, path)
      ? destination === null ? null : destination + path.slice(entry.path.length) : path;
    const drafts = new Map(this.drafts());
    const files = new Map<string, { original: string | null; content: string }>();
    for (const [path, draft] of drafts.get(id) ?? []) { const target = rewrite(path); if (target !== null) files.set(target, draft); }
    drafts.set(id, files);
    this.drafts.set(drafts);
    const folders = new Map(this.draftFolders());
    folders.set(id, new Set([...(folders.get(id) ?? [])].map(rewrite).filter((path): path is string => path !== null)));
    this.draftFolders.set(folders);
    const cache = new Map(this.directoryCache());
    const directories = new Map<string, SnapshotTreeEntry[]>();
    for (const [parent, entries] of cache.get(id) ?? []) {
      const target = rewrite(parent);
      if (target === null) continue;
      directories.set(target, entries.map((child) => {
        const path = rewrite(child.path);
        return path === null ? null : { ...child, path, name: path.split('/').pop()! };
      }).filter((child): child is SnapshotTreeEntry => child !== null && this.parentPath(child.path) === target));
    }
    if (destination !== null) {
      const parent = this.parentPath(destination);
      directories.set(parent, [...(directories.get(parent) ?? []), { ...entry, path: destination, name: destination.split('/').pop()! }]);
    }
    cache.set(id, directories);
    this.directoryCache.set(cache);
    this.expandedFolders.set(new Set([...this.expandedFolders()].map(rewrite).filter((path): path is string => path !== null)));
    const directory = rewrite(this.directoryPath());
    this.directoryPath.set(directory ?? this.parentPath(entry.path));
    const selected = this.selectedFilePath();
    if (selected !== null && this.contains(entry.path, selected)) {
      const path = rewrite(selected);
      this.fileSelection.next(path);
    }
  }

  private moveEntry(entry: SnapshotTreeEntry, destination: string) {
    if (destination === entry.path) { this.creationKind.set(null); return; }
    if (this.contains(entry.path, destination)) {
      this.creationError.set('A folder cannot be moved into itself.'); return;
    }
    if (new TextEncoder().encode(destination).length > 4096) {
      this.creationError.set('This path is too long.'); return;
    }
    const saved = this.editorSource();
    if (!saved || this.savingSnapshot() || this.movingEntry()) return;
    const parent = this.parentPath(destination);
    const apply = () => {
      if (this.children(parent).some((child) => child.path === destination)) {
        this.creationError.set('An entry with that name already exists.'); return;
      }
      if (!this.isNewEntry(entry)) this.appendOperation({ kind: 'move', path: entry.path, to: destination });
      this.rewriteDraftTree(entry, destination);
      this.expandPath(parent);
      this.creationKind.set(null);
      this.creationError.set('');
    };
    if (this.directoryCache().get(saved.id)?.has(parent) || this.draftFolders().get(saved.id)?.has(parent)) { apply(); return; }
    this.movingEntry.set(true);
    this.readTree(saved, this.originalPath(parent) ?? parent).pipe(
      takeUntil(this.loadedSnapshot.pipe(skip(1))), takeUntilDestroyed(this.destroyRef),
      finalize(() => this.movingEntry.set(false)),
    ).subscribe({
      next: (tree) => {
        this.cacheDirectory(saved.id, parent, tree.entries.map((child) => this.transformEntry(child)).filter((child): child is SnapshotTreeEntry => child !== null));
        apply();
      },
      error: (error: HttpErrorResponse) => { this.handleFileAccessError(error); this.creationError.set('Could not load the destination folder. Please try again.'); },
    });
  }
  protected dropOnEntry(event: DragEvent, entry: SnapshotTreeEntry) {
    event.preventDefault(); event.stopPropagation();
    if (entry.kind === 'directory') this.dropEntry(event, entry.path);
    else this.draggedPath = null;
  }
  protected treeKey(event: KeyboardEvent, entry: SnapshotTreeEntry) {
    const buttons = [...(event.currentTarget as HTMLElement).closest('[role="tree"]')!.querySelectorAll<HTMLButtonElement>('[role="treeitem"]')];
    const index = buttons.indexOf(event.currentTarget as HTMLButtonElement);
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault(); buttons[index + (event.key === 'ArrowDown' ? 1 : -1)]?.focus();
    } else if (event.key === 'ArrowRight' && entry.kind === 'directory' && !this.expandedFolders().has(entry.path)) {
      event.preventDefault(); this.openEntry(entry);
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault();
      if (entry.kind === 'directory' && this.expandedFolders().has(entry.path)) this.openEntry(entry);
      else buttons.find((button) => button.dataset['path'] === this.parentPath(entry.path))?.focus();
    }
  }
  protected startDrag(event: DragEvent, entry: SnapshotTreeEntry) {
    if (this.savingSnapshot() || this.movingEntry() || this.treeLoading()) { event.preventDefault(); return; }
    this.draggedPath = entry.path;
    event.dataTransfer?.setData('text/plain', entry.path);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
  }
  protected endDrag() { this.draggedPath = null; }
  protected allowDrop(event: DragEvent) {
    if (this.draggedPath === null || this.savingSnapshot() || this.movingEntry()) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
  }
  protected dropEntry(event: DragEvent, folder = '') {
    event.preventDefault(); event.stopPropagation();
    const path = this.draggedPath;
    this.draggedPath = null;
    const entry = this.treeRows().find((row) => row.path === path);
    if (!entry || this.savingSnapshot() || this.movingEntry()) return;
    this.moveEntry(entry, folder ? folder + '/' + entry.name : entry.name);
  }

  private handleFileAccessError(error: HttpErrorResponse) {
    if (error.status === 401) {
      this.api.clearSession();
      void this.router.navigateByUrl('/login');
    }
  }

  protected toggleMembers() {
    if (this.drawer() === 'members') {
      this.drawer.set(null);
    } else {
      this.drawer.set('members');
      this.retryMembers.next();
    }
  }

  protected loadMembers() {
    if (!this.membersLoading()) this.retryMembers.next();
  }
}

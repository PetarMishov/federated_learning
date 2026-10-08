import { DatePipe, DecimalPipe } from '@angular/common';
import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, OnInit, inject, signal } from '@angular/core';
import { takeUntilDestroyed, toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router } from '@angular/router';
import { BehaviorSubject, EMPTY, Subject, catchError, finalize, startWith, switchMap, tap } from 'rxjs';
import { Deployment, Member, Snapshot, SnapshotFile, SnapshotPreviewTooLarge, SnapshotTreeEntry, UsersApi } from '../users-api';

@Component({
  selector: 'app-project',
  imports: [DatePipe, DecimalPipe],
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
  private readonly loadedSnapshot = new BehaviorSubject<Snapshot | null>(null);
  private readonly directorySelection = new Subject<string>();
  private readonly fileSelection = new Subject<string | null>();
  protected readonly entries = signal<SnapshotTreeEntry[]>([]);
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
  protected readonly drawer = signal<'members' | 'deployments' | null>(null);
  protected readonly queryParams = toSignal(this.route.queryParamMap, {
    initialValue: this.route.snapshot.queryParamMap,
  });

  constructor() {
    this.destroyRef.onDestroy(() => clearTimeout(this.commitCopyTimeout));
  }

  ngOnInit() {
    this.loadedSnapshot.pipe(
      switchMap((saved) => {
        this.entries.set([]);
        this.directoryPath.set('');
        this.treeLoading.set(false);
        this.treeError.set('');
        if (!saved) return EMPTY;
        return this.directorySelection.pipe(
          startWith(''),
          switchMap((path) => {
            this.directoryPath.set(path);
            this.entries.set([]);
            this.treeError.set('');
            this.treeLoading.set(true);
            return this.api.getSnapshotTree(saved.project_id, saved.id, path).pipe(
              tap((tree) => this.entries.set(tree.entries)),
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
            return this.api.getSnapshotFile(saved.project_id, saved.id, path).pipe(
              tap((file) => this.file.set(file)),
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
        this.selectedSnapshotId.set(null);
        return this.snapshotSelection.pipe(
          startWith(null),
          switchMap((snapshotId) => {
            this.loadedSnapshot.next(null);
            this.snapshot.set(null);
            this.clearCommitCopyMessage();
            this.snapshotError.set('');
            this.snapshotLoading.set(false);
            if (snapshotId === null) return EMPTY;
            this.snapshotLoading.set(true);
            return this.api.getSnapshot(params.get('id') ?? '', snapshotId).pipe(
              tap((snapshot) => {
                this.snapshot.set(snapshot);
                this.loadedSnapshot.next(snapshot);
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

  protected closeSnapshot() {
    this.selectSnapshot(null);
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

  private clearCommitCopyMessage() {
    clearTimeout(this.commitCopyTimeout);
    this.commitCopyMessage.set('');
  }

  protected selectSnapshot(snapshotId: number | null) {
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
    if (entry.kind === 'directory') this.directorySelection.next(entry.path);
    else if (entry.kind === 'file') this.fileSelection.next(entry.path);
  }

  protected browseParent() {
    const parts = this.directoryPath().split('/');
    parts.pop();
    this.directorySelection.next(parts.join('/'));
  }

  protected loadTree() {
    if (!this.treeLoading()) this.directorySelection.next(this.directoryPath());
  }

  protected loadFile() {
    const path = this.selectedFilePath();
    if (path !== null && !this.fileLoading()) this.fileSelection.next(path);
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

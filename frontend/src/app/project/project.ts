import { DatePipe } from '@angular/common';
import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, OnInit, inject, signal } from '@angular/core';
import { takeUntilDestroyed, toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router } from '@angular/router';
import { EMPTY, Subject, catchError, finalize, startWith, switchMap, tap } from 'rxjs';
import { Deployment, Member, Snapshot, UsersApi } from '../users-api';

@Component({
  selector: 'app-project',
  imports: [DatePipe],
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
  protected readonly selectedDeployment = signal<Deployment | null>(null);
  protected readonly snapshot = signal<Snapshot | null>(null);
  protected readonly snapshotLoading = signal(false);
  protected readonly snapshotError = signal('');
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

  ngOnInit() {
    this.route.paramMap.pipe(
      switchMap((params) => {
        this.selectedDeployment.set(null);
        return this.snapshotSelection.pipe(
          startWith(null),
          switchMap((snapshotId) => {
            this.snapshot.set(null);
            this.snapshotError.set('');
            this.snapshotLoading.set(false);
            if (snapshotId === null) return EMPTY;
            this.snapshotLoading.set(true);
            return this.api.getSnapshot(params.get('id') ?? '', snapshotId).pipe(
              tap((snapshot) => this.snapshot.set(snapshot)),
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
    this.selectedDeployment.set(deployment);
    this.snapshotSelection.next(deployment.snapshot_id);
  }

  protected loadSnapshot() {
    const deployment = this.selectedDeployment();
    if (deployment && !this.snapshotLoading()) this.snapshotSelection.next(deployment.snapshot_id);
  }

  protected closeSnapshot() {
    this.selectedDeployment.set(null);
    this.snapshotSelection.next(null);
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

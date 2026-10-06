import { DatePipe } from '@angular/common';
import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { takeUntilDestroyed, toSignal } from '@angular/core/rxjs-interop';
import { EMPTY, Subject, catchError, finalize, forkJoin, map, of, startWith, switchMap, tap } from 'rxjs';
import { Deployment, Member, Project, UsersApi } from '../users-api';

type OrganizationDeployment = Deployment & { project_name: string };

@Component({
  selector: 'app-organization',
  imports: [RouterLink, DatePipe],
  templateUrl: './organization.html',
  styleUrl: './organization.css',
})
export class OrganizationPage implements OnInit {
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  private readonly api = inject(UsersApi);
  private readonly destroyRef = inject(DestroyRef);
  private readonly retry = new Subject<void>();
  private readonly retryMembers = new Subject<void>();
  private readonly retryDeployments = new Subject<void>();
  protected readonly deployments = signal<OrganizationDeployment[]>([]);
  protected readonly deploymentsLoading = signal(false);
  protected readonly deploymentsError = signal('');
  protected readonly members = signal<Member[]>([]);
  protected readonly membersLoading = signal(false);
  protected readonly membersError = signal('');
  protected readonly projects = signal<Project[]>([]);
  protected readonly loading = signal(false);
  protected readonly error = signal('');
  protected readonly routeParams = toSignal(this.route.paramMap);
  protected readonly queryParams = toSignal(this.route.queryParamMap, {
    initialValue: this.route.snapshot.queryParamMap,
  });

  ngOnInit() {
    this.route.paramMap.pipe(
      switchMap((params) => this.retry.pipe(
        startWith(undefined),
        switchMap(() => {
          this.projects.set([]);
          this.error.set('');
          this.loading.set(true);
          this.deployments.set([]);
          this.deploymentsError.set('');
          this.deploymentsLoading.set(true);
          return this.api.getOrganizationProjects(params.get('id') ?? '').pipe(
            tap((result) => this.projects.set(result.projects)),
            catchError((error: HttpErrorResponse) => {
              this.deploymentsLoading.set(false);
              this.deploymentsError.set('Could not load deployments because projects could not be loaded.');
              if (error.status === 401) {
                this.api.clearSession();
                void this.router.navigateByUrl('/login');
              } else {
                this.error.set(error.status === 404
                  ? 'Organization not found.'
                  : 'Could not load projects. Please try again.');
              }
              return EMPTY;
            }),
            finalize(() => this.loading.set(false)),
            switchMap((result) => this.retryDeployments.pipe(
              startWith(undefined),
              switchMap(() => this.fetchDeployments(result.projects)),
            )),
          );
        }),
      )),
      takeUntilDestroyed(this.destroyRef),
    ).subscribe();

    this.route.paramMap.pipe(
      switchMap((params) => this.retryMembers.pipe(
        startWith(undefined),
        switchMap(() => {
          this.members.set([]);
          this.membersError.set('');
          this.membersLoading.set(true);
          return this.api.getOrganizationMembers(params.get('id') ?? '').pipe(
            tap((result) => this.members.set(result.members)),
            catchError((error: HttpErrorResponse) => {
              if (error.status === 401) {
                this.api.clearSession();
                void this.router.navigateByUrl('/login');
              } else {
                this.membersError.set(error.status === 404
                  ? 'Organization not found.'
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
  }

  protected loadProjects() {
    if (!this.loading()) this.retry.next();
  }

  protected loadMembers() {
    if (!this.membersLoading()) this.retryMembers.next();
  }

  protected loadDeployments() {
    if (this.error()) this.loadProjects();
    else if (!this.deploymentsLoading()) this.retryDeployments.next();
  }

  private fetchDeployments(projects: Project[]) {
    this.deployments.set([]);
    this.deploymentsError.set('');
    this.deploymentsLoading.set(true);
    const request = projects.length
      ? forkJoin(projects.map((project) => this.api.getProjectDeployments(String(project.id)))).pipe(
          map((results) => results.flatMap((result, index) => result.deployments.map(
            (deployment) => ({ ...deployment, project_name: projects[index].name }),
          ))),
        )
      : of<OrganizationDeployment[]>([]);
    return request.pipe(
      tap((deployments) => this.deployments.set(
        deployments.sort((a, b) => b.created_at - a.created_at || b.id - a.id),
      )),
      catchError((error: HttpErrorResponse) => {
        if (error.status === 401) {
          this.api.clearSession();
          void this.router.navigateByUrl('/login');
        } else {
          this.deploymentsError.set('Could not load deployments. Please try again.');
        }
        return EMPTY;
      }),
      finalize(() => this.deploymentsLoading.set(false)),
    );
  }
}

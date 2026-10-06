import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, Router } from '@angular/router';
import { takeUntilDestroyed, toSignal } from '@angular/core/rxjs-interop';
import { EMPTY, Subject, catchError, finalize, startWith, switchMap, tap } from 'rxjs';
import { Project, UsersApi } from '../users-api';

@Component({
  selector: 'app-organization',
  templateUrl: './organization.html',
  styleUrl: './organization.css',
})
export class OrganizationPage implements OnInit {
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  private readonly api = inject(UsersApi);
  private readonly destroyRef = inject(DestroyRef);
  private readonly retry = new Subject<void>();
  protected readonly projects = signal<Project[]>([]);
  protected readonly loading = signal(false);
  protected readonly error = signal('');
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
          return this.api.getOrganizationProjects(params.get('id') ?? '').pipe(
            tap((result) => this.projects.set(result.projects)),
            catchError((error: HttpErrorResponse) => {
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
          );
        }),
      )),
      takeUntilDestroyed(this.destroyRef),
    ).subscribe();
  }

  protected loadProjects() {
    if (!this.loading()) this.retry.next();
  }
}

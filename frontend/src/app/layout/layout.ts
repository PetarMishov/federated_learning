import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, computed, inject, signal } from '@angular/core';
import { takeUntilDestroyed, toSignal } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router, RouterLink, RouterOutlet } from '@angular/router';
import { filter, finalize, map } from 'rxjs';
import { Organization, UsersApi } from '../users-api';

@Component({
  selector: 'app-layout',
  imports: [RouterLink, RouterOutlet],
  templateUrl: './layout.html',
  styleUrl: './layout.css',
  host: {
    '[class.project-layout]': 'isProjectPage()',
  },
})
export class Layout {
  private readonly api = inject(UsersApi);
  private readonly router = inject(Router);
  private readonly url = toSignal(this.router.events.pipe(
    filter((event): event is NavigationEnd => event instanceof NavigationEnd),
    map((event) => event.urlAfterRedirects),
  ), { initialValue: this.router.url });
  protected readonly isProjectPage = computed(() =>
    /^\/organizations\/[^/]+\/projects\/[^/?]+/.test(this.url()));
  private readonly destroyRef = inject(DestroyRef);
  protected readonly loggingOut = signal(false);
  protected readonly error = signal('');
  protected readonly organizations = signal<Organization[]>([]);
  protected readonly organizationsLoading = signal(false);
  protected readonly organizationsError = signal('');
  private organizationsLoaded = false;

  protected loadOrganizations() {
    if (this.organizationsLoaded || this.organizationsLoading()) return;
    this.organizationsLoading.set(true);
    this.organizationsError.set('');
    this.api.getOrganizations().pipe(
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.organizationsLoading.set(false)),
    ).subscribe({
      next: (result) => {
        this.organizations.set(result.organizations);
        this.organizationsLoaded = true;
      },
      error: (error: HttpErrorResponse) => {
        if (error.status === 401) this.finishLogout();
        else this.organizationsError.set('Could not load organizations.');
      },
    });
  }

  protected logout() {
    if (this.loggingOut()) return;
    this.loggingOut.set(true);
    this.error.set('');
    this.api.logout().pipe(
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.loggingOut.set(false)),
    ).subscribe({
      next: () => this.finishLogout(),
      error: (error: HttpErrorResponse) => {
        if (error.status === 401) this.finishLogout();
        else this.error.set('Could not log out. Please try again.');
      },
    });
  }

  private finishLogout() {
    this.api.clearSession();
    void this.router.navigateByUrl('/login');
  }
}

import { HttpErrorResponse } from '@angular/common/http';
import { Component, DestroyRef, ElementRef, HostListener, computed, effect, inject, input, output, signal } from '@angular/core';
import { Router } from '@angular/router';
import { EMPTY, Subscription, catchError, expand, finalize, tap } from 'rxjs';
import { Repository, RepositoryProvider, UsersApi } from '../users-api';

@Component({
  selector: 'app-repository-picker',
  templateUrl: './repository-picker.html',
  styleUrl: './repository-picker.css',
})
export class RepositoryPicker {
  readonly provider = input.required<RepositoryProvider>();
  readonly selectionChange = output<Repository | null>();
  private readonly api = inject(UsersApi);
  private readonly router = inject(Router);
  private readonly element = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly destroyRef = inject(DestroyRef);
  private request: Subscription | undefined;
  private focusTimer: ReturnType<typeof setTimeout> | undefined;
  protected readonly open = signal(false);
  protected readonly loading = signal(false);
  protected readonly error = signal('');
  protected readonly query = signal('');
  protected readonly repositories = signal<Repository[]>([]);
  protected readonly selected = signal<Repository | null>(null);
  protected readonly activeIndex = signal(0);
  protected readonly filtered = computed(() => {
    const query = this.query().trim().toLowerCase();
    return this.repositories().filter((repo) => repo.full_name.toLowerCase().includes(query));
  });

  constructor() {
    effect(() => {
      this.provider();
      this.request?.unsubscribe();
      this.open.set(false);
      this.repositories.set([]);
      this.selected.set(null);
      this.query.set('');
      this.error.set('');
      this.activeIndex.set(0);
      this.selectionChange.emit(null);
    });
    this.destroyRef.onDestroy(() => {
      this.request?.unsubscribe();
      clearTimeout(this.focusTimer);
    });
  }

  protected toggle() {
    if (this.open()) { this.close(); return; }
    this.open.set(true);
    if (!this.repositories().length && !this.loading()) this.load();
    this.focusTimer = setTimeout(() => this.element.nativeElement.querySelector<HTMLInputElement>('input')?.focus());
  }

  protected load() {
    this.request?.unsubscribe();
    this.error.set('');
    this.loading.set(true);
    this.repositories.set([]);
    const provider = this.provider();
    this.request = this.api.getRepositories(provider).pipe(
      expand((page, index) => page.has_more ? this.api.getRepositories(provider, index + 2) : EMPTY),
      tap((page) => {
        const merged = new Map(this.repositories().map((repo) => [repo.id, repo]));
        for (const repo of page.repositories) merged.set(repo.id, repo);
        this.repositories.set([...merged.values()].sort((a, b) => a.full_name.localeCompare(b.full_name, undefined, { sensitivity: 'base' })));
      }),
      catchError((error: HttpErrorResponse) => {
        if (error.status === 401) {
          this.api.clearSession();
          void this.router.navigateByUrl('/login');
        } else {
          this.error.set(error.status === 404
            ? `Add a ${provider === 'github' ? 'GitHub' : 'GitLab'} token before selecting a repository.`
            : error.status === 403 ? 'Your token was rejected. Replace it with a valid read token.'
            : 'Could not load repositories. Please try again.');
        }
        return EMPTY;
      }),
      finalize(() => this.loading.set(false)),
    ).subscribe();
  }

  protected search(value: string) { this.query.set(value); this.activeIndex.set(0); }

  protected choose(repository: Repository) {
    this.selected.set(repository);
    this.selectionChange.emit(repository);
    this.close(true);
  }

  protected dismiss(event: Event) {
    event.preventDefault();
    event.stopPropagation();
    this.close(true);
  }

  protected key(event: KeyboardEvent) {
    if (event.key === 'Escape') { this.dismiss(event); }
    else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const count = this.filtered().length;
      if (!count) return;
      this.activeIndex.update((index) => (index + (event.key === 'ArrowDown' ? 1 : -1) + count) % count);
      this.element.nativeElement.querySelector(`#repository-option-${this.provider()}-${this.activeIndex()}`)?.scrollIntoView?.({ block: 'nearest' });
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const repository = this.filtered()[this.activeIndex()];
      if (repository) this.choose(repository);
    }
  }

  private close(focusTrigger = false) {
    this.open.set(false);
    clearTimeout(this.focusTimer);
    if (focusTrigger) this.element.nativeElement.querySelector<HTMLButtonElement>('.repository-trigger')?.focus();
  }

  @HostListener('document:click', ['$event'])
  protected outsideClick(event: MouseEvent) {
    if (!this.element.nativeElement.contains(event.target as Node)) this.close();
  }

  @HostListener('focusout', ['$event'])
  protected focusOut(event: FocusEvent) {
    if (event.relatedTarget && !this.element.nativeElement.contains(event.relatedTarget as Node)) this.close();
  }
}

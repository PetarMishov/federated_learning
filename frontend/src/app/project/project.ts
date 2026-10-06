import { Component, inject, signal } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute } from '@angular/router';

@Component({
  selector: 'app-project',
  templateUrl: './project.html',
  styleUrl: './project.css',
})
export class ProjectPage {
  private readonly route = inject(ActivatedRoute);
  protected readonly source = signal<'github' | 'gitlab' | 'local'>('github');
  protected readonly drawer = signal<'members' | 'deployments' | null>(null);
  protected readonly queryParams = toSignal(this.route.queryParamMap, {
    initialValue: this.route.snapshot.queryParamMap,
  });
}
